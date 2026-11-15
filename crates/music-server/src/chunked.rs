//! Fetching a large file over several connections at once.
//!
//! One connection per file is what the studio used to do, and on a twelve
//! gigabyte model that is one TCP stream deciding how long the user waits -
//! usually a small fraction of the line. Hugging Face serves ranges, so the
//! file is cut into pieces and several are fetched at a time.
//!
//! The shape of this is taken from Dub Studio, which learned it the hard way,
//! and the awkward parts of it are the point:
//!
//! * Four connections for the whole job, not four per file and not sixteen.
//!   Xet-CAS - where the alternative quantisations live - drops connections
//!   under heavier parallelism, and a dropped range leaves a hole in a file
//!   whose size is already correct, so nothing notices until the model fails
//!   to load.
//! * A range is only accepted when the server answers 206 and sends exactly
//!   the bytes asked for. Anything else is retried, up to eight times, with the
//!   progress of the failed attempt subtracted again.
//! * A piece counts as done only after its bytes are on the disk - `sync_data`
//!   first, then its offset is appended to the manifest beside the file. The
//!   other order survives a normal exit and loses to a power cut: the manifest
//!   would promise data that never landed, and the resumed download would skip
//!   a hole.
//!
//! * One counted request per file. Hugging Face counts every request to
//!   `/resolve/` as a download and answers it with a redirect to a signed CDN
//!   address. The probe follows that redirect once and the pieces go to where it
//!   led, so a 3.8 GB file is one download on the model's page, not the 230 its
//!   pieces used to be (#249). The signed address runs out after about an hour;
//!   the first piece that meets the refusal resolves it again, once.
//!
//! Where a server does not serve ranges, this falls back to reading the whole
//! body in order, which is what the studio did everywhere before.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;

/// How much of a file one task fetches. Small enough that a dropped connection
/// costs little, large enough that the queue is not mostly bookkeeping.
const CHUNK: u64 = 16 * 1024 * 1024;

/// Connections for the whole studio - not per file and not per panel.
///
/// Four is not shyness. Xet-CAS starts dropping connections above that, and a
/// dropped range is a hole in a file of exactly the right size. Four per file
/// looked fine until two panels downloaded at once: eight connections, and the
/// second download sat at nought bytes while the first ran. Hence one budget,
/// held by whoever is fetching a piece right now.
const SLOTS: usize = 4;

fn connections() -> &'static tokio::sync::Semaphore {
    static BUDGET: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    BUDGET.get_or_init(|| tokio::sync::Semaphore::new(SLOTS))
}

/// Attempts per piece before the download gives up. Transient drops are normal
/// on a twelve gigabyte file - seven hundred pieces - and a retry is cheap.
const RETRIES: u32 = 8;

/// How long a download may spend waiting to be let in before it gives up.
///
/// "Too many requests" is not a failure, it is a queue, and it must not count
/// against the attempts above: eight retries four hundred milliseconds apart
/// gave up after fourteen seconds and told the user their download was broken.
/// Hugging Face's limits clear in minutes, so the download waits minutes.
const RATE_LIMIT_BUDGET: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// Seconds the studio is currently waiting for a server to let it in, so a
/// download that is queued rather than broken can say so. A bar at nought per
/// cent with nothing under it is indistinguishable from a hang.
static WAITING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long the download is being kept waiting, if it is.
pub fn waiting_for_server() -> Option<u64> {
    match WAITING.load(Ordering::Relaxed) {
        0 => None,
        seconds => Some(seconds),
    }
}

/// What Hugging Face says about the window we are in.
///
/// It does not send `Retry-After`. It implements the IETF RateLimit header
/// draft instead: `RateLimit: "resolvers";r=2871;t=143` - requests left in this
/// window, and seconds until it resets. Waiting on a guess when the server has
/// told you the number is how a download sits for fifteen seconds and walks
/// straight back into the same refusal.
fn window(response: &reqwest::Response) -> Option<Window> {
    let header = response.headers().get("ratelimit")?.to_str().ok()?;
    let field = |name: &str| {
        header
            .split(';')
            .filter_map(|part| part.trim().split_once('='))
            .find(|(key, _)| key.trim() == name)
            .and_then(|(_, value)| value.trim().parse::<u64>().ok())
    };
    Some(Window { remaining: field("r")?, resets_in: field("t").unwrap_or(300) })
}

#[derive(Debug, Clone, Copy)]
struct Window {
    remaining: u64,
    resets_in: u64,
}

/// Below this many requests left in the window, a download stops asking and
/// waits for the window to turn over. Hitting nought means every request -
/// including the ones already in flight - comes back 429, which is much more
/// expensive than pausing for a minute.
const KEEP_IN_RESERVE: u64 = 25;

/// How long to wait when the window is spent, or when the server refused
/// without saying why.
fn asked_to_wait(response: &reqwest::Response) -> std::time::Duration {
    let seconds = window(response).map(|window| window.resets_in).unwrap_or(30);
    std::time::Duration::from_secs(seconds.clamp(1, 310))
}

/// Whether a status is the server asking for patience rather than refusing.
fn is_busy(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// The address each resolved download was asked for, by the address it was
/// redirected to: a signed CDN address runs out, and only the original can be
/// resolved again.
fn origins() -> &'static Mutex<HashMap<String, String>> {
    static ORIGINS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    ORIGINS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember(resolved: &str, asked: &str) {
    if resolved != asked {
        if let Ok(mut map) = origins().lock() {
            // an address resolved from another resolved one keeps the first original
            let first = map.get(asked).cloned().unwrap_or_else(|| asked.to_owned());
            map.insert(resolved.to_owned(), first);
        }
    }
}

fn origin_of(resolved: &str) -> Option<String> {
    origins().lock().ok().and_then(|map| map.get(resolved).cloned())
}

/// A redirected address the server no longer honours (its signature ran out):
/// resolved again, not retried as it is.
#[derive(Debug)]
struct Expired(String);

impl std::fmt::Display for Expired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Expired {}

/// What the server will let us do with this file.
#[derive(Debug, Clone, Copy)]
pub struct Plan {
    pub total: u64,
    pub ranged: bool,
}

/// Asks for one byte, which answers both questions at once: how big the file
/// is, and whether ranges work at all. A HEAD answers the first only, and some
/// CDNs answer it differently from the GET that follows.
///
/// The address returned is where the server's redirect led (the url itself when
/// there was none): `fetch` takes its pieces from there, so the Hub counts the
/// file once.
pub async fn probe(http: &reqwest::Client, url: &str) -> Result<(Plan, String)> {
    let mut last = String::new();
    let mut unreachable = 0;
    let mut attempts = 0;
    for attempt in 0..RETRIES {
        attempts = attempt + 1;
        match probe_once(http, url).await {
            Ok((plan, resolved)) => {
                remember(&resolved, url);
                return Ok((plan, resolved));
            }
            Err(error) if error.is::<Refused>() => return Err(error),
            Err(error) => {
                last = format!("{error:#}");
                if cannot_connect(&error) {
                    unreachable += 1;
                    if unreachable == CONNECT_RETRIES {
                        break;
                    }
                }
            }
        }
        // Hugging Face answers 429 when it has had enough for the moment, and
        // a busy hour is not a broken download. Without this the whole install
        // died on the first refusal, with nothing to retry it.
        tokio::time::sleep(std::time::Duration::from_millis(500 << attempt.min(7))).await;
    }
    // Hugging Face could not be reached from this network at all - the top setup
    // failure in telemetry, a network behind the Great Firewall among them. The
    // HF-Mirror usually answers where it does not, so fall back to it once before
    // giving up; a non-HF url mirrors to itself and drops straight through.
    // Through a proxy whose address takes no connection the mirror is behind the same dead proxy: the proxy is named
    // instead. A proxy that takes the connection and cannot reach the host is alive, and the mirror is tried through it.
    let proxy = match crate::net::route_for(url).filter(|_| unreachable == CONNECT_RETRIES) {
        Some(route) if !route.takes_connections().await => Some(route),
        _ => None,
    };
    if unreachable == CONNECT_RETRIES && proxy.is_none() {
        let mirror = crate::net::mirror_url(url);
        if mirror != url {
            if let Ok(found) = Box::pin(probe(http, &mirror)).await {
                return Ok(found);
            }
        }
    }
    Err(anyhow!("{}", start_failure(url, attempts, unreachable == CONNECT_RETRIES, &last, proxy.as_ref())))
}

/// Attempts at a host that cannot be connected to at all - no route, no DNS
/// answer, a refused or timed-out connection - before the download says so.
/// Each costs up to the connect timeout, and a blocked host stays blocked.
const CONNECT_RETRIES: u32 = 3;

/// Whether the request never reached the server: a connection that failed, not
/// an answer and not a dropped transfer.
fn cannot_connect(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.downcast_ref::<reqwest::Error>().is_some_and(|error| error.is_connect()))
}

/// Why a download could not start. The host is named on its own, so it
/// survives the statistics' removal of addresses: which server a network
/// cannot reach is the whole question.
fn start_failure(url: &str, attempts: u32, unreachable: bool, last: &str, proxy: Option<&crate::net::ProxyRoute>) -> String {
    let host = reqwest::Url::parse(url).ok().and_then(|parsed| parsed.host_str().map(str::to_owned)).unwrap_or_default();
    if let Some(proxy) = proxy.filter(|_| unreachable) {
        return format!("could not connect to {host} ({attempts} attempts) for {url} through a proxy: {last}. {}", proxy.did_not_connect());
    }
    let mirror = if host == "huggingface.co" {
        ". Hugging Face did not answer from this network: Settings - Network can take models from the HF-Mirror instead"
    } else {
        ""
    };
    if unreachable {
        format!("could not connect to {host} ({attempts} attempts) for {url}: {last}{mirror}")
    } else {
        format!("could not start {url} from {host} after {attempts} attempts: {last}{mirror}")
    }
}

/// Sends one request, waiting out a server that is only busy.
///
/// The wait comes from the server's own `Retry-After` where it gives one. This
/// is where "too many requests" stops being an error: the caller sees either a
/// real answer or a real failure, never a queue.
async fn send(http: &reqwest::Client, url: &str, range: Option<String>) -> Result<reqwest::Response> {
    let mut waited = std::time::Duration::ZERO;
    loop {
        let _slot = connections().acquire().await.context("wait for a connection")?;
        let mut request = http.get(url);
        if let Some(range) = &range {
            request = request.header(reqwest::header::RANGE, range);
        }
        let response = request.send().await.with_context(|| format!("request {url}"))?;
        if !is_busy(response.status()) {
            // Slow down before the wall, not after it. The window is five
            // minutes long and shared by every request from this machine, so a
            // download that spends the last of it earns a refusal for
            // everything else the studio is doing.
            if let Some(window) = window(&response) {
                if window.remaining < KEEP_IN_RESERVE {
                    let pause = std::time::Duration::from_secs(window.resets_in.clamp(1, 310));
                    drop(_slot);
                    WAITING.store(pause.as_secs(), Ordering::Relaxed);
                    tokio::time::sleep(pause).await;
                    WAITING.store(0, Ordering::Relaxed);
                }
            }
            return Ok(response);
        }
        let pause = asked_to_wait(&response);
        if waited + pause > RATE_LIMIT_BUDGET {
            bail!("{url} answered {} for {} minutes", response.status(), waited.as_secs() / 60);
        }
        waited += pause;
        // The permit is dropped before sleeping, so a server that is turning
        // us away does not also hold the studio's connections shut.
        drop(_slot);
        WAITING.store(pause.as_secs().max(1), Ordering::Relaxed);
        tokio::time::sleep(pause).await;
        WAITING.store(0, Ordering::Relaxed);
    }
}

async fn probe_once(http: &reqwest::Client, url: &str) -> Result<(Plan, String)> {
    let response = send(http, url, Some("bytes=0-0".into())).await?;
    let resolved = response.url().to_string();
    if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let total = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if total > 0 {
            return Ok((Plan { total, ranged: true }, resolved));
        }
    }
    if response.status().is_client_error() {
        return Err(Refused(format!("{url} answered {}", response.status())).into());
    }
    if !response.status().is_success() {
        bail!("{url} answered {}", response.status());
    }
    Ok((Plan { total: response.content_length().unwrap_or(0), ranged: false }, resolved))
}

/// A signed address ran out under a piece: the first piece to meet it resolves
/// the original again, and the rest find the new address already in place.
async fn refresh(http: &reqwest::Client, current: &tokio::sync::Mutex<String>, failed: &str) -> Result<()> {
    let mut url = current.lock().await;
    if *url != failed {
        return Ok(());
    }
    let origin = origin_of(failed).ok_or_else(|| anyhow!("{failed} ran out and has no address to resolve it again from"))?;
    let (_, fresh) = probe_once(http, &origin).await?;
    remember(&fresh, &origin);
    *url = fresh;
    Ok(())
}

/// A server that said no (a 4xx; "too many requests" is waited out in `send`): asking again gets the same answer.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// The offsets already on disk, from the manifest beside the part file.
pub fn manifest_path(part: &Path) -> PathBuf {
    let mut name = part.as_os_str().to_os_string();
    name.push(".done");
    PathBuf::from(name)
}

/// How much of a download lies in its part file: the pieces its manifest names, the part file being reserved at
/// the whole size; a part file without a manifest (a server without ranges) holds what it is long.
pub fn on_disk(part: &Path) -> u64 {
    let Ok(length) = std::fs::metadata(part).map(|meta| meta.len()) else { return 0 };
    if !manifest_path(part).exists() {
        return length;
    }
    let done: std::collections::HashSet<u64> = completed_offsets(part).into_iter().filter(|offset| *offset < length).collect();
    done.iter().map(|offset| CHUNK.min(length - offset)).sum()
}

fn completed_offsets(part: &Path) -> Vec<u64> {
    let Ok(bytes) = std::fs::read(manifest_path(part)) else { return Vec::new() };
    bytes
        .chunks_exact(8)
        .map(|entry| u64::from_le_bytes([entry[0], entry[1], entry[2], entry[3], entry[4], entry[5], entry[6], entry[7]]))
        .collect()
}

#[cfg(windows)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> std::io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(buffer, offset)
}

#[cfg(not(windows))]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(buffer, offset)
}

/// Fetches `url` into `part`, several pieces at a time, and reports progress by
/// adding to `written`.
///
/// `written` starts at whatever the resumed pieces already account for, so the
/// caller can add several files' counters together and get a figure that only
/// ever goes up.
pub async fn fetch(
    http: &reqwest::Client,
    url: &str,
    part: &Path,
    plan: Plan,
    written: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    if let Some(parent) = part.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if !plan.ranged || plan.total == 0 {
        return whole(http, url, part, written, cancel).await;
    }

    let file = Arc::new(
        OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(part)
            .with_context(|| format!("open {}", part.display()))?,
    );
    file.set_len(plan.total).with_context(|| format!("reserve {} bytes", plan.total))?;

    let done: std::collections::HashSet<u64> = completed_offsets(part).into_iter().filter(|offset| *offset < plan.total).collect();
    let manifest = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(manifest_path(part))
            .with_context(|| format!("open {}", manifest_path(part).display()))?,
    ));

    let mut pieces = Vec::new();
    let mut start = 0u64;
    while start < plan.total {
        let end = (start + CHUNK - 1).min(plan.total - 1);
        if done.contains(&start) {
            written.fetch_add(end - start + 1, Ordering::Relaxed);
        } else {
            pieces.push((start, end));
        }
        start += CHUNK;
    }
    if pieces.is_empty() {
        return Ok(());
    }

    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(pieces)));
    // Shared, not copied: a signed address resolved again by one piece is the
    // address every other piece takes next.
    let current = Arc::new(tokio::sync::Mutex::new(url.to_string()));
    // The caller's cancel flag is the stop flag: a piece that fails stops the
    // rest, and so does the user pressing cancel.
    let stop = cancel;
    let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    // Tasks, not connections: the semaphore above decides how many of them are
    // talking to a server at any moment, so a second download gets a turn.
    let workers = (0..SLOTS.min(queue.lock().map(|queue| queue.len()).unwrap_or(1)).max(1)).map(|_| {
        let (http, current, file, queue, written, stop, failure, manifest) = (
            http.clone(),
            current.clone(),
            file.clone(),
            queue.clone(),
            written.clone(),
            stop.clone(),
            failure.clone(),
            manifest.clone(),
        );
        async move {
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let Some((start, end)) = ({ let mut queue = queue.lock().unwrap(); queue.pop_front() }) else { return };
                if let Err(error) = piece(&http, &current, &file, start, end, &written, &stop, &manifest).await {
                    stop.store(true, Ordering::Relaxed);
                    *failure.lock().unwrap() = Some(error.to_string());
                    return;
                }
            }
        }
    });
    futures_util::future::join_all(workers).await;

    if let Some(error) = failure.lock().unwrap().take() {
        // The part file and its manifest stay: what did arrive is on the disk,
        // and the next attempt fetches only what did not.
        bail!(error);
    }
    file.sync_all().ok();
    std::fs::remove_file(manifest_path(part)).ok();
    Ok(())
}

/// One piece, with the retries that make a long download survive a bad line.
async fn piece(
    http: &reqwest::Client,
    current: &tokio::sync::Mutex<String>,
    file: &Arc<File>,
    start: u64,
    end: u64,
    written: &Arc<AtomicU64>,
    stop: &Arc<AtomicBool>,
    manifest: &Arc<Mutex<File>>,
) -> Result<()> {
    let wanted = end - start + 1;
    let mut last = String::new();
    for attempt in 0..RETRIES {
        if stop.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let mut got = 0u64;
        let url = current.lock().await.clone();
        let outcome = once(http, &url, file, start, end, written, &mut got, stop).await;
        match outcome {
            Ok(()) if got == wanted => {
                // The bytes reach the disk before the manifest says they did.
                // The other order loses a power cut: a resumed download would
                // skip a piece that is a hole.
                file.sync_data().ok();
                if let Ok(mut manifest) = manifest.lock() {
                    manifest.write_all(&start.to_le_bytes()).ok();
                    manifest.sync_data().ok();
                }
                return Ok(());
            }
            Ok(()) => last = format!("incomplete range: {got} of {wanted} bytes"),
            Err(error) => {
                if stop.load(Ordering::Relaxed) {
                    bail!("cancelled");
                }
                last = error.to_string();
                if error.is::<Expired>() {
                    // resolved again at once: the wait below is for a bad line, not for a signature
                    if let Err(again) = refresh(http, current, &url).await {
                        last = format!("{last}; resolving it again: {again:#}");
                    } else {
                        continue;
                    }
                }
            }
        }
        // The failed attempt's bytes are subtracted again, or the bar would
        // count them twice when the retry fetches the same range.
        written.fetch_sub(got.min(written.load(Ordering::Relaxed)), Ordering::Relaxed);
        // Exponential, as the Hub's own download protocol asks for: half a
        // second, then one, two, four - eight attempts spanning a minute
        // instead of the three seconds a linear back-off gave a network that
        // needs a moment to come back.
        tokio::time::sleep(std::time::Duration::from_millis(500 << attempt.min(7))).await;
    }
    Err(anyhow!("range {start}-{end} failed after {RETRIES} attempts: {last}"))
}

async fn once(
    http: &reqwest::Client,
    url: &str,
    file: &Arc<File>,
    start: u64,
    end: u64,
    written: &Arc<AtomicU64>,
    got: &mut u64,
    stop: &Arc<AtomicBool>,
) -> Result<()> {
    let response = send(http, url, Some(format!("bytes={start}-{end}"))).await?;
    if matches!(response.status().as_u16(), 401 | 403 | 410) && origin_of(url).is_some() {
        return Err(Expired(format!("range {start}-{end}: the redirected address answered {}", response.status())).into());
    }
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        bail!("range {start}-{end}: answered {} where 206 was expected", response.status());
    }
    let mut stream = response.bytes_stream();
    let mut offset = start;
    while let Some(chunk) = stream.next().await {
        // Checked here and not only between pieces: four sixteen megabyte
        // pieces in flight is ten seconds of downloading after the user has
        // pressed stop, which reads as a button that did nothing.
        if stop.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let chunk = chunk.context("read range")?;
        let mut placed = 0usize;
        while placed < chunk.len() {
            let count = write_at(file, &chunk[placed..], offset + placed as u64).context("write range")?;
            if count == 0 {
                bail!("short write at {}", offset + placed as u64);
            }
            placed += count;
        }
        offset += chunk.len() as u64;
        *got += chunk.len() as u64;
        written.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    Ok(())
}

/// For a server that will not serve ranges: the body, in order, from the start.
async fn whole(http: &reqwest::Client, url: &str, part: &Path, written: Arc<AtomicU64>, cancel: Arc<AtomicBool>) -> Result<()> {
    let response = send(http, url, None).await?;
    if !response.status().is_success() {
        bail!("{url} answered {}", response.status());
    }
    let mut file = File::create(part).with_context(|| format!("create {}", part.display()))?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let chunk = chunk.context("read download chunk")?;
        file.write_all(&chunk).context("write download chunk")?;
        written.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.flush().context("flush download")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_reads_back_the_offsets_it_was_given() {
        let directory = std::env::temp_dir().join(format!("yue2-chunked-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let part = directory.join("model.gguf.part");
        std::fs::write(manifest_path(&part), [0u64, CHUNK, CHUNK * 2].iter().flat_map(|offset| offset.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
        assert_eq!(completed_offsets(&part), vec![0, CHUNK, CHUNK * 2]);
        std::fs::remove_dir_all(&directory).ok();
    }

    /// The header as Hugging Face actually sends it, copied from a live
    /// response rather than from the specification: what matters is what comes
    /// down the wire.
    #[test]
    fn the_rate_limit_header_is_read_the_way_the_hub_writes_it() {
        let header = "\"resolvers\";r=2819;t=216";
        let field = |name: &str| {
            header
                .split(';')
                .filter_map(|part| part.trim().split_once('='))
                .find(|(key, _)| key.trim() == name)
                .and_then(|(_, value)| value.trim().parse::<u64>().ok())
        };
        assert_eq!(field("r"), Some(2819), "requests left in this five minute window");
        assert_eq!(field("t"), Some(216), "seconds until the window resets");
    }

    #[test]
    fn a_missing_manifest_means_nothing_is_done_yet() {
        assert!(completed_offsets(Path::new("nowhere/at/all.part")).is_empty());
    }

    /// A part file is reserved at the whole size: what is on disk is the pieces its manifest names, the last one
    /// shorter; without a manifest the part file holds what it is long.
    #[test]
    fn a_part_file_holds_the_pieces_its_manifest_names() {
        let folder = std::env::temp_dir().join(format!("on-disk-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&folder).unwrap();
        let part = folder.join("model.gguf.part");
        let total = CHUNK * 2 + 10;
        std::fs::File::create(&part).unwrap().set_len(total).unwrap();
        assert_eq!(on_disk(&part), total, "no manifest: a download without ranges");
        let offsets: Vec<u8> = [0u64, CHUNK * 2].iter().flat_map(|offset| offset.to_le_bytes()).collect();
        std::fs::write(manifest_path(&part), offsets).unwrap();
        assert_eq!(on_disk(&part), CHUNK + 10);
        assert_eq!(on_disk(&folder.join("nothing.part")), 0);
        std::fs::remove_dir_all(&folder).ok();
    }

    /// A host nothing listens on is given up after three attempts, not eight,
    /// and the reason names it beside the address.
    #[tokio::test]
    async fn a_host_that_cannot_be_reached_is_named_and_not_retried_eight_times() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let error = probe(&http, &format!("http://127.0.0.1:{port}/model.gguf")).await.unwrap_err().to_string();
        assert!(error.starts_with(&format!("could not connect to 127.0.0.1 (3 attempts) for http://127.0.0.1:{port}/model.gguf: ")), "{error}");
        assert!(!error.contains("HF-Mirror"));
        assert!(start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 3, true, "x", None).starts_with("could not connect to huggingface.co (3 attempts)"));
        assert!(start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 3, true, "x", None).ends_with("HF-Mirror instead"));
        assert_eq!(start_failure("https://github.com/a/b.zip", 8, false, "x", None), "could not start https://github.com/a/b.zip from github.com after 8 attempts: x");
    }

    #[test]
    fn a_proxy_that_does_not_connect_is_named_and_the_mirror_is_not_offered() {
        let system = crate::net::ProxyRoute { address: "http://127.0.0.1:10809".into(), system: true };
        let through = start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 3, true, "tunnel error", Some(&system));
        assert!(through.contains(". The system proxy http://127.0.0.1:10809 did not answer"), "{through}");
        assert!(through.contains("choose No proxy in Settings - Providers"), "{through}");
        assert!(!through.contains("HF-Mirror"), "the mirror sits behind the same proxy: {through}");
        let own = crate::net::ProxyRoute { address: "socks5h://proxy.example:1080".into(), system: false };
        assert!(start_failure("https://github.com/a/b.zip", 3, true, "x", Some(&own)).contains("The proxy set in Settings - Providers socks5h://proxy.example:1080"));
        let answered = start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 8, false, "x", Some(&system));
        assert!(answered.ends_with("HF-Mirror instead") && !answered.contains("proxy"), "a server that answered through the proxy is not the proxy's fault: {answered}");
    }

    /// A file served behind a redirect, as the Hub serves one: `/resolve/` answers 302 to a signed address, which serves ranges. Counts the
    /// requests to `/resolve/`; the signed address stops being honoured after `honour` ranges when `honour` is set.
    async fn hub_with_cdn(body: Arc<Vec<u8>>, honour: Option<usize>) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use axum::http::{header, HeaderMap, StatusCode};
        use axum::response::IntoResponse;
        use std::sync::atomic::AtomicUsize;
        let resolves = Arc::new(AtomicUsize::new(0));
        let served = Arc::new(AtomicUsize::new(0));
        let (counted, cdn_served) = (resolves.clone(), served.clone());
        let app = axum::Router::new()
            .route(
                "/m/resolve/rev/model.gguf",
                axum::routing::get(move || {
                    let token = counted.fetch_add(1, Ordering::SeqCst) + 1;
                    async move { (StatusCode::FOUND, [(header::LOCATION, format!("/cdn/model.gguf?token={token}"))]).into_response() }
                }),
            )
            .route(
                "/cdn/model.gguf",
                axum::routing::get(move |query: axum::extract::RawQuery, headers: HeaderMap| {
                    let (body, served) = (body.clone(), cdn_served.clone());
                    async move {
                        let token: usize = query.0.unwrap_or_default().trim_start_matches("token=").parse().unwrap_or(0);
                        let n = served.fetch_add(1, Ordering::SeqCst);
                        if honour.is_some_and(|honour| token == 1 && n >= honour) {
                            return StatusCode::FORBIDDEN.into_response();
                        }
                        let range = headers.get(header::RANGE).and_then(|value| value.to_str().ok()).unwrap_or("bytes=0-");
                        let (a, b) = range.trim_start_matches("bytes=").split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = b.parse().unwrap_or(body.len() - 1).min(body.len() - 1);
                        (
                            StatusCode::PARTIAL_CONTENT,
                            [(header::CONTENT_RANGE, format!("bytes {a}-{b}/{}", body.len()))],
                            body[a..=b].to_vec(),
                        )
                            .into_response()
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });
        (format!("http://{address}/m/resolve/rev/model.gguf"), resolves)
    }

    fn body_of(length: usize) -> Arc<Vec<u8>> {
        Arc::new((0..length).map(|i| (i * 31 + i / 7) as u8).collect())
    }

    /// #249: a file of three pieces is one request to `/resolve/`; its pieces come from the address the redirect led to.
    #[tokio::test]
    async fn a_redirected_file_is_resolved_once_and_its_pieces_come_from_where_it_led() {
        let body = body_of(CHUNK as usize * 2 + 123);
        let (url, resolves) = hub_with_cdn(body.clone(), None).await;
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let (plan, resolved) = probe(&http, &url).await.unwrap();
        assert!(plan.ranged && plan.total == body.len() as u64);
        assert!(resolved.contains("/cdn/model.gguf?token=1"), "{resolved}");
        let folder = std::env::temp_dir().join(format!("resolve-once-{}", uuid::Uuid::now_v7()));
        let part = folder.join("model.gguf.part");
        fetch(&http, &resolved, &part, plan, Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false))).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), *body);
        assert_eq!(resolves.load(Ordering::SeqCst), 1, "one counted request for the whole file");
        std::fs::remove_dir_all(&folder).ok();
    }

    /// A signed address that runs out mid-file is resolved again once, and the download finishes from the new one.
    #[tokio::test]
    async fn a_signed_address_that_ran_out_is_resolved_again_once() {
        let body = body_of(CHUNK as usize * 4 + 9);
        let (url, resolves) = hub_with_cdn(body.clone(), Some(2)).await;
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let (plan, resolved) = probe(&http, &url).await.unwrap();
        let folder = std::env::temp_dir().join(format!("resolve-again-{}", uuid::Uuid::now_v7()));
        let part = folder.join("model.gguf.part");
        fetch(&http, &resolved, &part, plan, Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false))).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), *body);
        assert_eq!(resolves.load(Ordering::SeqCst), 2, "the probe, and once more when the signature ran out");
        std::fs::remove_dir_all(&folder).ok();
    }
}
