use std::{collections::BTreeMap, env, fs::File, io::{BufRead, Read, Seek, SeekFrom, Write}};

use hyper::{body::Buf, client};
use hyper_rustls::ConfigBuilderExt;
use regex::Regex;

struct DotnetDiagnostic {
    code: String,
    message: String,
    severity: String,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn fetch_url(url: hyper::Uri) -> Result<impl Buf> {
    let tls = rustls::ClientConfig::builder().with_safe_defaults().with_native_roots().with_no_client_auth();
    let https = hyper_rustls::HttpsConnectorBuilder::new().with_tls_config(tls).https_only().enable_http1().build();
    let client: client::Client<_, hyper::Body> = client::Client::builder().build(https);
    let response = client.get(url).await?;
    return Ok(hyper::body::aggregate(response).await?);
}

async fn get_roslyn_error_codes(branch: &String) -> Result<BTreeMap<String, i32>> {
    let mut error_codes: BTreeMap<String, i32> = BTreeMap::new();
    let uri = format!("https://raw.githubusercontent.com/dotnet/roslyn/{branch}/src/Compilers/CSharp/Portable/Errors/ErrorCode.cs", branch = branch);
    println!("[roslyn] uri: {}", uri);

    let text = fetch_url(uri.parse()?).await?.reader().lines();
    let mut stage = 0;
    println!("[roslyn] parsing error codes");
    for line in text {
        let line = line?;
        if stage == 0 {
            if line.contains("=") && line.contains("_") {
                stage = 1;
            }
        }

        if stage == 1 {
            if line.contains("}") {
                break;
            }

            let line = line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with("#") {
                continue;
            }

            let parts = line.split("=").collect::<Vec<&str>>();
            if parts.len() != 2 {
                continue;
            }

            let codeword = parts[0].trim();
            let code = parts[1].trim().split(",").collect::<Vec<&str>>()[0].parse::<i32>();
            if code.is_err() {
                println!("[roslyn] error parsing code: {} = {}", parts[0].trim(), parts[1].trim());
            }
            error_codes.insert(codeword.to_string(), code?);
        }
    }

    println!("[roslyn] found {} error codes", error_codes.len());
    return Ok(error_codes)
}

async fn get_roslyn_resx_map(branch: &String) -> Result<BTreeMap<String, String>> {
    let mut resx_map: BTreeMap<String, String> = BTreeMap::new();
    let uri = format!("https://raw.githubusercontent.com/dotnet/roslyn/{branch}/src/Compilers/CSharp/Portable/CSharpResources.resx", branch = branch);
    println!("[roslyn] uri: {}", uri);

    let text = fetch_url(uri.parse()?).await?.reader();
    println!("[roslyn] parsing resx data");

    let mut reader = quick_xml::reader::Reader::from_reader(text);
    reader.trim_text(true);

    let mut txt = String::new();
    let mut buf = Vec::new();
    let mut name = String::new();
    let mut stage = 0;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                match e.name().as_ref() {
                    b"data" => {
                        for attr in e.attributes() {
                            let attr = attr?;
                            if attr.key.as_ref() == b"name" {
                                stage = 1;
                                name = String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                    },
                    b"value" => {
                        if stage == 1 {
                            stage = 2;
                            txt = String::new();
                        }
                    },
                    _ => (),
                }
            }
            Ok(quick_xml::events::Event::Text(e)) => {
                if stage == 2 {
                    txt += &e.unescape()?.into_owned();
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                if stage == 2 && e.name().as_ref() == b"value" {
                    stage = 0;
                    resx_map.insert(name.clone(), txt.clone());
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
            _ => (),
        }
        buf.clear();
    }

    println!("[roslyn] found {} resx messages", resx_map.len());
    return Ok(resx_map);
}

async fn get_roslyn_analyzer_codes(branch: &String, map: &mut BTreeMap<String, DotnetDiagnostic>) -> Result<()> {
    let uri = format!("https://raw.githubusercontent.com/dotnet/roslyn-analyzers/{branch}/src/NetAnalyzers/Microsoft.CodeAnalysis.NetAnalyzers.sarif", branch = branch);
    println!("[roslyn-analyzers] uri: {}", uri);

    let mut text = fetch_url(uri.parse()?).await?.reader();
    println!("[roslyn-analyzers] parsing sarif data");

    let mut text_data: Vec<u8> = Vec::new();
    text.read_to_end(&mut text_data)?;
    if text_data.len() == 0 {
        return Ok(());
    }

    if text_data[0] == 0xEF && text_data[1] == 0xBB && text_data[2] == 0xBF {
        println!("[roslyn-analyzers] BOM?");
        text_data = text_data[3..].to_vec();
    }

    let data = json::parse(String::from_utf8_lossy(&text_data).to_string().as_str())?;
    for run in data["runs"].members() {
        for (_, run) in run["rules"].entries() {
            let id = run["id"].as_str().unwrap();
            let message = run["shortDescription"].as_str().unwrap();
            let severity = run["defaultLevel"].as_str().unwrap();
            let diagnostic = DotnetDiagnostic {
                code: id.to_string(),
                message: message.to_string(),
                severity: match severity {
                    "fatal" => "error",
                    "error" => "error",
                    "warning" => "warning",
                    "information" => "suggestion",
                    "note" => "suggestion",
                    "hidden" => "none",
                    _ => panic!("unknown severity: {}", severity),
                }.to_string(),
            };
            map.insert(id.to_string().to_lowercase(), diagnostic);
        };
    }

    return Ok(());
}

// tested on roslyn-e59309f35553d53147088c01c5b7706d1e8cdec2
// tested on roslyn-analyzers-b7bb138809d5a7d31508fe0cd86d59ed4c864764
#[tokio::main]
async fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 2 {
        println!("Usage: editorconfig-updater <path/to/existing/.editorconfig> [roslyn-branch=main] [roslyn-analyzers-branch=main]");
        return Err("not enough arguments".into());
    }

    let default = String::from("main");
    let mut diagnostics: BTreeMap<String, DotnetDiagnostic> = BTreeMap::new();

    // get roslyn error codes
    {
        let roslyn_branch = args.get(2).unwrap_or(&default);
        println!("[roslyn] branch {}", roslyn_branch);
        let error_codes = get_roslyn_error_codes(&roslyn_branch).await?;
        let resx_map = get_roslyn_resx_map(&roslyn_branch).await?;

        let unknown = String::from("Unknown");
        for (code, id) in error_codes.iter() {
            let message = resx_map.get(code).unwrap_or(&unknown);
            let formatted = format!("CS{:0>4}", id);
            diagnostics.insert(formatted.clone().to_lowercase(), DotnetDiagnostic {
                code: formatted.clone(),
                message: message.clone(),
                severity: match code.get(0..3).ok_or("HDN")? {
                    "FTL" => "error", // fatal
                    "ERR" => "error",
                    "WRN" => "warning",
                    "INF" => "suggestion", // information
                    "HDN" => "none", // hidden
                    _ => panic!("unknown severity: {}", code),
                }.to_string(),
            });
        }
    }

    // get roslyn analyzer codes
    {
        let roslyn_analyzers_branch = args.get(3).unwrap_or(&default);
        println!("[roslyn-analyzers] branch {}", roslyn_analyzers_branch);
        get_roslyn_analyzer_codes(&roslyn_analyzers_branch, &mut diagnostics).await?;
    }

    let re = Regex::new(r"\s+").unwrap();

    // read existing severities and write file
    {
        let path = args.get(1).unwrap();
        println!("updating severities for {}", path);
        let mut contents = String::new();

        // read the entire file

        let mut file = File::options().read(true).write(true).open(path)?;
        file.read_to_string(&mut contents)?;
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;

        let lines = contents.lines().collect::<Vec<_>>();
        for line in &lines {
            if !line.trim().starts_with("dotnet_diagnostic.") {
                continue;
            }

            let parts = line.split("#").collect::<Vec<_>>()[0].split("=").collect::<Vec<_>>();
            let code = parts[0].trim().split(".").collect::<Vec<_>>()[1];
            let override_severity = parts[1].trim();
            let diagnostic = diagnostics.get(code);
            if diagnostic.is_none() {
                println!("unknown diagnostic: {}", code);
                continue;
            }

            let diagnostic = diagnostic.unwrap();
            if diagnostic.severity != override_severity {
                println!("updating {} from {} to {}", code, diagnostic.severity, override_severity);
            }

            let diagnostic = DotnetDiagnostic {
                code: diagnostic.code.clone(),
                message: diagnostic.message.clone(),
                severity: override_severity.to_string(),
            };
            diagnostics.insert(code.to_string(), diagnostic);
        }

        let mut written = 0;
        for original_line in &lines {
            let line = original_line.trim();
            if !line.starts_with("dotnet_diagnostic.") {
                file.write(line.as_bytes())?;
                file.write(b"\n")?;
                continue;
            }

            if written > 0 {
                continue;
            }

            written = 1;
            for (code, diagnostic) in &diagnostics {
                println!("writing {}", code);
                let cleaned_message = re.replace_all(&diagnostic.message.replace("\r\n", " ").replace("\n", " "), " ").trim().to_string();
                file.write_fmt(format_args!("dotnet_diagnostic.{}.severity = {} # {}\n", diagnostic.code.to_lowercase(), diagnostic.severity, cleaned_message))?;
            }
        }
    }
    return Ok(());
}
