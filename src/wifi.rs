use core::net::Ipv4Addr;

use embassy_executor::Spawner;
use embassy_net::{Config as NetConfig, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4};
use embassy_time::{Duration, Timer};
use esp_hal::efuse::{self, MacAddress};
use esp_hal::peripherals::WIFI;
use esp_hal::rng::Rng;
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config as WifiConfig, Interface, Password, Ssid, WifiController,
    WifiError,
    sta::{DisconnectedInfo, StationConfig},
};
use static_cell::StaticCell;

pub struct Network {
    pub ssid: &'static str,
    pub pass: &'static str,
    pub ip: Option<StaticIp>,
}

#[derive(Clone, Copy)]
pub struct StaticIp {
    pub addr: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub prefix_len: u8,
}

pub struct Settings {
    pub mac: Option<[u8; 6]>,
    pub retry_delay_secs: u64,
}

include!(concat!(env!("OUT_DIR"), "/wifi_generated.rs"));

static RESOURCES: StaticCell<StackResources<SOCKET_COUNT>> = StaticCell::new();

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) {
    runner.run().await
}

pub struct Wifi {
    controller: WifiController<'static>,
    stack: Stack<'static>,
}

impl Wifi {
    pub async fn connect(
        peripheral: WIFI<'static>,
        spawner: Spawner,
        settings: &Settings,
        networks: &'static [Network],
    ) -> Result<Self, WifiError> {
        if let Some(mac) = settings.mac {
            efuse::override_mac_address(MacAddress::new_eui48(mac)).ok();
        }

        let mut controller = WifiController::new(peripheral, Default::default())?;
        let station = Interface::station();
        log::info!("station mac {:?}", station.mac_address());

        let network = 'connect: loop {
            for network in networks {
                log::info!("trying {}", network.ssid);
                let sta = StationConfig::default()
                    .with_ssid(Ssid::try_from(network.ssid)?)
                    .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                        Password::try_from(network.pass)?,
                    ));
                if let Err(e) = controller.set_config(&WifiConfig::Station(sta)) {
                    log::warn!("configure {} failed: {:?}", network.ssid, e);
                    continue;
                }
                match controller.connect_async().await {
                    Ok(_) => break 'connect network,
                    Err(e) => log::warn!("connect {} failed: {:?}", network.ssid, e),
                }
            }
            log::warn!("no connection; retry in {}s", settings.retry_delay_secs);
            Timer::after(Duration::from_secs(settings.retry_delay_secs)).await;
        };
        log::info!("connected to {}", network.ssid);

        let rng = Rng::new();
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;
        let (stack, runner) = embassy_net::new(
            station,
            net_config(network),
            RESOURCES.init(StackResources::new()),
            seed,
        );
        spawner.spawn(net_task(runner).expect("spawn net task"));
        stack.wait_config_up().await;
        log::info!("network up: {:?}", stack.config_v4());

        Ok(Self { controller, stack })
    }

    pub fn stack(&self) -> Stack<'static> {
        self.stack
    }

    pub async fn wait_for_disconnect(&self) -> Result<DisconnectedInfo, WifiError> {
        self.controller.wait_for_disconnect_async().await
    }
}

fn net_config(network: &Network) -> NetConfig {
    match network.ip {
        None => NetConfig::dhcpv4(Default::default()),
        Some(StaticIp {
            addr,
            gateway,
            prefix_len,
        }) => NetConfig::ipv4_static(StaticConfigV4 {
            address: Ipv4Cidr::new(addr, prefix_len),
            gateway: Some(gateway),
            dns_servers: Default::default(),
        }),
    }
}
