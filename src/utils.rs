use core::str;

use crate::{CONFIG_ADDR, ID_ADDR, SECRET_ADDR};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use anyhow::anyhow;
use bleps::HciConnector;
use embedded_storage::{ReadStorage, Storage};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::peripherals::{DAC2, GPIO2, GPIO26, GPIO4};
use esp_hal::rng::Rng;
use esp_hal::system::software_reset;
use esp_hal::time;
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use esp_storage::FlashStorage;
use esp_wifi::ble::controller::BleConnector;
use esp_wifi::init;
use esp_wifi::wifi::{WifiController, WifiDevice};
use smoltcp::iface::Interface;
use smoltcp::wire::{IpAddress, Ipv4Address};

pub fn init_hardware<'a>() -> (
    Rng,
    HciConnector<BleConnector<'static>>,
    WifiController<'static>,
    Interface,
    WifiDevice<'a>,
    GPIO26<'a>,
    GPIO2<'a>,
    GPIO4<'a>,
    DAC2<'a>,
) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let rng = Rng::new(peripherals.RNG);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    // Will leak memory if used more than once
    let esp_wifi_ctrl = Box::leak(Box::new(
        init(timg0.timer0, rng, peripherals.RADIO_CLK).unwrap(),
    ));
    let connector = BleConnector::new(esp_wifi_ctrl, peripherals.BT);
    let hci = HciConnector::new(connector, now);
    let (mut controller, interfaces) =
        esp_wifi::wifi::new(esp_wifi_ctrl, peripherals.WIFI).unwrap();

    let mut device = interfaces.sta;
    let iface = create_interface(&mut device);
    let gpio26 = peripherals.GPIO26;
    let gpio2 = peripherals.GPIO2;
    let gpio4 = peripherals.GPIO4;
    let dac2 = peripherals.DAC2;
    controller
        .set_power_saving(esp_wifi::config::PowerSaveMode::None)
        .unwrap();
    (
        rng, hci, controller, iface, device, gpio26, gpio2, gpio4, dac2,
    )
}
pub fn actual_ip(ip: &str) -> [u8; 4] {
    let vec: Vec<u8> = ip
        .split('.')
        .map(|num| match num.to_string().parse::<u8>() {
            Err(_) => {
                panic!("Ip address is wrong");
            }
            Ok(x) => x,
        })
        .collect();
    vec.as_slice().try_into().unwrap()
}
pub fn get_env() -> (u16, IpAddress, bool) {
    let ip_env: &str = env!("IP");
    let debug_env: bool = match option_env!("DEBUG") {
        Some(val) => val.parse::<bool>().expect("Invalid DEBUG value"),
        None => false,
    };
    println!(env!("PORT"));
    // Line currently required for DEVICE_SECRET to appear as a string

    let port: u16 = env!("PORT")
        .parse::<u16>()
        .expect("PORT is not a valid port");
    let ip_address_bytes = actual_ip(ip_env);
    let ip_address = IpAddress::Ipv4(Ipv4Address::new(
        ip_address_bytes[0],
        ip_address_bytes[1],
        ip_address_bytes[2],
        ip_address_bytes[3],
    ));
    (port, ip_address, debug_env)
}
pub fn get_device_data(fs: &mut FlashStorage) -> (String, String) {
    let device_id_bytes = get_device_id(fs);
    let device_id = str::from_utf8(&device_id_bytes).unwrap();
    println!("{}", device_id);

    // Device secret is 344 bytes long
    let device_secret_bytes = get_device_secret(fs);
    // Converting with utf-8 resulted in errors in printable characters
    let device_secret = device_secret_bytes.as_ascii().unwrap().as_str();
    println!("{}", device_secret);
    (String::from(device_id), String::from(device_secret))
}
pub fn is_device_configured(fs: &mut FlashStorage) -> bool {
    let mut config_bytes = [255u8; 4];
    fs.read(CONFIG_ADDR, &mut config_bytes).unwrap();
    config_bytes == [0, 0, 0, 0]
}

pub fn now() -> u64 {
    time::Instant::now().duration_since_epoch().as_millis()
}
pub fn create_interface(device: &mut WifiDevice) -> Interface {
    // users could create multiple instances but since they only have one WifiDevice
    // they probably can't do anything bad with that
    Interface::new(
        smoltcp::iface::Config::new(smoltcp::wire::HardwareAddress::Ethernet(
            smoltcp::wire::EthernetAddress::from_bytes(&device.mac_address()),
        )),
        device,
        timestamp(),
    )
}
fn timestamp() -> smoltcp::time::Instant {
    smoltcp::time::Instant::from_micros(
        time::Instant::now().duration_since_epoch().as_micros() as i64
    )
}

pub fn get_device_id(fs: &mut FlashStorage) -> [u8; 36] {
    let mut buf: [u8; 36] = [0u8; 36];
    // Device ID is 36 bytes long
    fs.read(ID_ADDR, &mut buf).unwrap();
    buf
}
pub fn get_device_secret(fs: &mut FlashStorage) -> [u8; 344] {
    let mut secret_buf: [u8; 344] = [0u8; 344];
    fs.read(SECRET_ADDR, &mut secret_buf).unwrap();
    secret_buf
}

/// No point in returing anything since this resets the whole chip
// TODO consider wiping wifi credentials
pub fn handle_device_reset(fs: &mut FlashStorage) {
    let config_bytes = [0xff, 0xff, 0xff, 0xff];
    fs.write(CONFIG_ADDR, &config_bytes).unwrap();
    software_reset(); //maybe use software_reset_cpu
}
#[allow(dead_code)]
pub fn set_random_mac(mut rng: Rng) -> Result<(), anyhow::Error> {
    let mut fake_mac: [u8; 6] = [0u8; 6];

    for fake_byte in fake_mac.iter_mut() {
        *fake_byte = rng.random() as u8;
    }
    match esp_hal::efuse::Efuse::set_mac_address(fake_mac) {
        Ok(()) => Ok(()),
        Err(_) => Err(anyhow!("Mac error")),
    }
}
