use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
};

fn main() {
    let args: Vec<_> = env::args().collect();
    let port = if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--config") {
        let config = fs::read_to_string(&pair[1]).unwrap();
        config
            .split_once(r#""port":"#)
            .unwrap()
            .1
            .trim_start()
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse::<u16>()
            .unwrap()
    } else {
        args.windows(2).find(|pair| pair[0] == "--port").unwrap()[1]
            .parse::<u16>()
            .unwrap()
    };
    let address = ("127.0.0.1", port);
    let listener = TcpListener::bind(address).unwrap();
    let mut count = 0;
    for client in listener.incoming().flatten() {
        let _ = handle(client, &mut count);
    }
}

fn handle(mut client: TcpStream, count: &mut u32) -> std::io::Result<()> {
    let mut input = BufReader::new(&mut client);
    let mut request = String::new();
    input.read_line(&mut request)?;
    let Some((method, path)) = request
        .split_whitespace()
        .next()
        .zip(request.split_whitespace().nth(1))
    else {
        return Ok(());
    };
    let mut len = 0;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                len = value.trim().parse().unwrap();
            }
        }
    }
    let mut body = vec![0; len];
    input.read_exact(&mut body)?;
    let (status, mime, bytes) = match (method, path) {
        ("GET", "/health") => ("200 OK", "application/json", b"{}".to_vec()),
        ("POST", "/v1/tasks/run") => {
            fs::write("audio-cpp-submitted.json", body)?;
            (
                "200 OK",
                "application/json",
                fs::read("audio-cpp-response.json")?,
            )
        }
        ("POST", "/v1/tasks/unload_models") => {
            fs::write("audio-cpp-unloaded.json", body)?;
            (
                "200 OK",
                "application/json",
                br#"{"unloaded":["minimax_music3"],"not_found":[]}"#.to_vec(),
            )
        }
        ("GET", "/props") => (
            "200 OK",
            "application/json",
            br#"{"models":{
            "lm":["mm3-lm-Q3_K_M.gguf"],"depth":["mm3-depth-Q4_K_M.gguf"],
            "cond":["MiniMax-Music3-condition_encoder-F32.gguf"],"dit":["mm3-dit-Q3_K_M.gguf"],
            "vae":["MiniMax-Music3-vocoder-F32.gguf"]}}"#
                .to_vec(),
        ),
        ("POST", "/synth") => {
            *count += 1;
            fs::write(format!("submitted-{count}.json"), body)?;
            (
                "200 OK",
                "application/json",
                format!(r#"{{"id":"{count}"}}"#).into_bytes(),
            )
        }
        ("GET", "/job?id=1" | "/job?id=2") => (
            "200 OK",
            "application/json",
            br#"{"status":"done"}"#.to_vec(),
        ),
        ("GET", "/job?id=3") => (
            "200 OK",
            "application/json",
            br#"{"status":"running"}"#.to_vec(),
        ),
        ("POST", "/job?id=3&cancel=1") => {
            fs::write("cancelled", "3")?;
            (
                "200 OK",
                "application/json",
                br#"{"status":"cancelled"}"#.to_vec(),
            )
        }
        ("GET", "/job?id=1&result=1" | "/job?id=2&result=1") => {
            let replay = path == "/job?id=2&result=1";
            fs::write("fetched", if replay { "2" } else { "1" })?;
            let mut response =
                format!("--fixture\r\nContent-Type: application/json\r\n\r\n{{\"audio_codes\":\"1,2,3\",\"seed\":{}}}\r\n--fixture\r\nContent-Type: audio/wav\r\n\r\n", if replay { 43 } else { 42 }).into_bytes();
            response.extend(fs::read("fixture.wav")?);
            response.extend(b"\r\n--fixture--\r\n");
            ("200 OK", "multipart/mixed; boundary=fixture", response)
        }
        _ => (
            "404 Not Found",
            "application/json",
            br#"{"error":"unexpected test request"}"#.to_vec(),
        ),
    };
    write!(
        client,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )?;
    client.write_all(&bytes)
}
