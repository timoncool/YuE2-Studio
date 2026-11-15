//! When an agent last used the studio.
//!
//! The desktop shell stops a studio it started for an agent (`--headless`) once nothing has
//! used it for a while. The studio's own window cannot count as that use: open or hidden, it
//! keeps asking for songs, jobs and settings, and a studio judged by those requests would
//! never stop. The window says where it is in every request - its page as the Origin of what
//! it fetches and of its event streams, and as the Referer of what it loads (a song to play, a
//! cover) - and that page is the bundled one or, in development, the dev server. An agent
//! sends neither; a browser elsewhere, or one on this computer that opened the studio at the
//! service's own address, sends that address, and the person using it counts as use too. MCP
//! is always an agent: the window never calls it. The MCP tools reach the routes inside the
//! process, past this layer, so a tool call counts once, as the call to `/mcp` it is.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::{header, HeaderMap};
use axum::middleware::Next;
use axum::response::Response;

fn last_call() -> &'static Mutex<Option<Instant>> {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    &LAST
}

/// Agents' calls not answered yet: a lyrics request to a slow model can outlast the idle period.
static OPEN_CALLS: AtomicUsize = AtomicUsize::new(0);

fn note() {
    *last_call().lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
}

/// An agent's call for as long as it is open, one the agent gave up on included.
struct OpenCall;

impl OpenCall {
    fn start() -> Self {
        note();
        OPEN_CALLS.fetch_add(1, Ordering::SeqCst);
        OpenCall
    }
}

impl Drop for OpenCall {
    fn drop(&mut self) {
        note();
        OPEN_CALLS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How long ago an agent last called the service: nothing while a call is open, none while no
/// agent has called.
pub fn quiet_for() -> Option<Duration> {
    if OPEN_CALLS.load(Ordering::SeqCst) > 0 {
        return Some(Duration::ZERO);
    }
    let last = *last_call().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    last.map(|at| at.elapsed())
}

/// Counts a call that is not the window's own from when it comes until it is answered, so a
/// call that waits a minute for a job is use for that whole minute.
pub async fn track(request: Request, next: Next) -> Response {
    let _call = by_an_agent(request.uri().path(), request.headers()).then(OpenCall::start);
    next.run(request).await
}

fn by_an_agent(path: &str, headers: &HeaderMap) -> bool {
    if path == "/mcp" {
        return true;
    }
    ![header::ORIGIN, header::REFERER]
        .iter()
        .filter_map(|name| headers.get(name))
        .filter_map(|value| value.to_str().ok())
        .any(page_of_the_window)
}

/// The window's page: the bundled one (`tauri://localhost`, `http://tauri.localhost`) or the
/// development server, a page of this computer on another port than the service's.
fn page_of_the_window(address: &str) -> bool {
    let Ok(page) = reqwest::Url::parse(address) else { return false };
    let host = page.host_str().unwrap_or_default();
    page.scheme() == "tauri"
        || host.eq_ignore_ascii_case("tauri.localhost")
        || (crate::remote::loopback_host(host) && page.port_or_known_default() != Some(crate::listen_port()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name.clone(), value.parse().unwrap());
        }
        headers
    }

    #[test]
    fn the_window_is_not_an_agent_and_everything_else_is() {
        let service = format!("http://127.0.0.1:{}/", crate::listen_port());
        assert!(by_an_agent("/v1/music/jobs", &headers(&[])), "curl, a script, an MCP client");
        assert!(!by_an_agent("/v1/music/jobs", &headers(&[(header::ORIGIN, "http://tauri.localhost")])));
        assert!(!by_an_agent("/v1/music/jobs", &headers(&[(header::ORIGIN, "tauri://localhost")])));
        assert!(!by_an_agent("/mcp/window", &headers(&[(header::ORIGIN, "http://tauri.localhost")])), "the window's command stream");
        assert!(!by_an_agent("/v1/library/songs", &headers(&[(header::REFERER, "http://127.0.0.1:3791/")])), "the development server");
        assert!(!by_an_agent("/v1/media/song.mp3", &headers(&[(header::REFERER, "http://tauri.localhost/")])), "a song the window plays");
        assert!(by_an_agent("/v1/music/jobs", &headers(&[(header::ORIGIN, "http://192.168.1.5:8791")])), "a browser on another computer");
        assert!(by_an_agent("/v1/music/jobs", &headers(&[(header::REFERER, service.as_str())])), "a browser here, at the service's own address");
        assert!(by_an_agent("/v1/music/jobs", &headers(&[(header::ORIGIN, "null")])), "not an address at all");
        assert!(by_an_agent("/mcp", &headers(&[(header::ORIGIN, "http://localhost:6274")])), "MCP is never the window");
    }

    #[test]
    fn a_call_is_use_while_open_and_remembered_after() {
        let call = OpenCall::start();
        assert_eq!(quiet_for(), Some(Duration::ZERO));
        drop(call);
        assert!(quiet_for().is_some_and(|quiet| quiet < Duration::from_secs(5)));
    }
}
