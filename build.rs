use std::path::PathBuf;

use serde::Deserialize;

const CONFIG_FILE: &str = "wifi_config.toml";
const EXAMPLE_FILE: &str = "wifi_config.toml.example";

fn main() {
    generate_wifi_config();

    linker_be_nice();
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(default)]
    settings: RawSettings,
    network: Vec<RawNetwork>,
}

#[derive(Deserialize)]
struct RawSettings {
    mac: Option<String>,
    #[serde(default = "default_retry_delay")]
    retry_delay_secs: u64,
    #[serde(default = "default_socket_count")]
    socket_count: usize,
}

impl Default for RawSettings {
    fn default() -> Self {
        Self {
            mac: None,
            retry_delay_secs: default_retry_delay(),
            socket_count: default_socket_count(),
        }
    }
}

fn default_retry_delay() -> u64 {
    60
}

fn default_socket_count() -> usize {
    4
}

#[derive(Deserialize)]
struct RawNetwork {
    ssid: String,
    pass: String,
    ip: Option<RawIp>,
}

#[derive(Deserialize)]
struct RawIp {
    ip: String,
    gateway: String,
    subnet: String,
}

fn generate_wifi_config() {
    println!("cargo:rerun-if-changed={CONFIG_FILE}");

    let source = std::fs::read_to_string(CONFIG_FILE).unwrap_or_else(|e| {
        panic!(
            "{CONFIG_FILE} could not be read ({e}). Copy {EXAMPLE_FILE} to {CONFIG_FILE} and fill in your networks."
        );
    });
    let config: RawConfig = toml::from_str(&source).expect("parse wifi config");

    let mut out = String::new();
    out.push_str(&format!(
        "pub const SOCKET_COUNT: usize = {};\n\n",
        config.settings.socket_count
    ));
    out.push_str("pub static SETTINGS: Settings = Settings {\n");
    out.push_str(&format!("    mac: {},\n", mac_literal(&config.settings.mac)));
    out.push_str(&format!(
        "    retry_delay_secs: {},\n",
        config.settings.retry_delay_secs
    ));
    out.push_str("};\n\n");
    out.push_str("pub static NETWORKS: &[Network] = &[\n");
    for network in &config.network {
        assert!(
            network.ssid.len() <= 32,
            "ssid {:?} exceeds 32 bytes",
            network.ssid
        );
        assert!(
            network.pass.len() <= 64,
            "password for {:?} exceeds 64 bytes",
            network.ssid
        );
        out.push_str("    Network {\n");
        out.push_str(&format!("        ssid: {:?},\n", network.ssid));
        out.push_str(&format!("        pass: {:?},\n", network.pass));
        match &network.ip {
            None => out.push_str("        ip: None,\n"),
            Some(ip) => out.push_str(&format!(
                "        ip: Some(StaticIp {{ addr: {}, gateway: {}, prefix_len: {} }}),\n",
                ipv4_literal(&ip.ip),
                ipv4_literal(&ip.gateway),
                prefix_len(&ip.subnet),
            )),
        }
        out.push_str("    },\n");
    }
    out.push_str("];\n");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("wifi_generated.rs"), out).unwrap();
}

fn mac_literal(mac: &Option<String>) -> String {
    match mac {
        None => "None".to_string(),
        Some(mac) => {
            let bytes: Vec<u8> = mac
                .split(':')
                .map(|b| u8::from_str_radix(b, 16).expect("invalid mac byte"))
                .collect();
            assert_eq!(bytes.len(), 6, "mac must have 6 octets");
            format!(
                "Some([{}])",
                bytes
                    .iter()
                    .map(|b| format!("0x{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn ipv4_literal(ip: &str) -> String {
    let addr: std::net::Ipv4Addr = ip.parse().expect("invalid ipv4 address");
    let [a, b, c, d] = addr.octets();
    format!("Ipv4Addr::new({a}, {b}, {c}, {d})")
}

fn prefix_len(subnet: &str) -> u8 {
    let mask = u32::from(
        subnet
            .parse::<std::net::Ipv4Addr>()
            .expect("invalid subnet mask"),
    );
    let prefix = mask.leading_ones() as u8;
    assert_eq!(
        mask,
        if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) },
        "subnet mask must be contiguous"
    );
    prefix
}
