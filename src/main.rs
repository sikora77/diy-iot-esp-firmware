#![feature(ascii_char)]
#![feature(iter_collect_into)]
#![no_std]
#![no_main]
extern crate alloc;

use crate::utils::{
    get_device_id, get_device_secret, get_wifi_config, handle_device_reset, init_wifi,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use anyhow::anyhow;
use bleps::HciConnector;
use blocking_network_stack::Stack;
use embedded_storage::ReadStorage;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::gpio::{Input, Io, Level, Output, OutputConfig, Pull};
use esp_hal::{clock::CpuClock, main, rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_storage::FlashStorage;
use esp_wifi::{ble::controller::BleConnector, init};
use serde::{Deserialize, Serialize};
use smoltcp::wire::Ipv4Address;
use smoltcp::{
    iface::{SocketSet, SocketStorage},
    wire::{DhcpOption, IpAddress},
};
use utils::now;
esp_bootloader_esp_idf::esp_app_desc!();

mod coap;
mod errors;
mod pairing;
mod utils;

const CONFIG_ADDR: u32 = 0x9000;
const SSID_ADDR: u32 = 0x9080;
const PASS_ADDR: u32 = 0x9080 + 128;
const ID_ADDR: u32 = 0x9080 + 256;
const SECRET_ADDR: u32 = ID_ADDR + 36;

#[derive(Serialize, Deserialize, Clone)]
pub struct LightState {
    pub is_on: bool,
    pub brightness: u8,
    pub color: i32,
    pub removed: bool,
}

// #[global_allocator]
// static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();
//
// fn init_heap() {
//     const HEAP_SIZE: usize = 32 * 1024;
//     static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();
//     unsafe {
//         ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
//     }
// }
//
fn actual_ip(ip: &str) -> [u8; 4] {
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
fn get_env() -> (u16, [u8; 4], bool) {
    let ip_env: &str = core::env!("IP");
    let debug_env: bool = match core::option_env!("DEBUG") {
        Some(val) => val.parse::<bool>().expect("Invalid DEBUG value"),
        None => false,
    };
    println!(core::env!("PORT"));
    // Line currently required for DEVICE_SECRET to appear as a string

    let port: u16 = core::env!("PORT")
        .parse::<u16>()
        .expect("PORT is not a valid port");
    let ip_address = actual_ip(ip_env);
    (port, ip_address, debug_env)
}
fn get_device_data(fs: &mut FlashStorage) -> (String, String) {
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
fn is_device_configured(fs: &mut FlashStorage) -> bool {
    let mut config_bytes = [255u8; 4];
    fs.read(CONFIG_ADDR, &mut config_bytes).unwrap();
    config_bytes == [0, 0, 0, 0]
}
#[main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 24 * 1024);
    let mut fs = FlashStorage::new();
    let (port_env, ip_address, _debug_env) = get_env();

    let (device_id, _device_secret) = get_device_data(&mut fs);
    let mut rng = Rng::new(peripherals.RNG);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let esp_wifi_ctrl = init(timg0.timer0, rng, peripherals.RADIO_CLK).unwrap();
    let connector = BleConnector::new(&esp_wifi_ctrl, peripherals.BT);
    let hci = HciConnector::new(connector, now);
    // Initializing wifi
    let (mut controller, interfaces) =
        esp_wifi::wifi::new(&esp_wifi_ctrl, peripherals.WIFI).unwrap();

    let mut device = interfaces.sta;
    let iface = create_interface(&mut device);

    controller
        .set_power_saving(esp_wifi::config::PowerSaveMode::None)
        .unwrap();

    let mut socket_set_entries: [SocketStorage; 3] = Default::default();
    let mut socket_set = SocketSet::new(&mut socket_set_entries[..]);
    let mut dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
    // we can set a hostname here (or add other DHCP options)
    dhcp_socket.set_outgoing_options(&[DhcpOption {
        kind: 12,
        data: b"esp-wifi",
    }]);
    socket_set.add(dhcp_socket);

    let stack = Stack::new(iface, device, socket_set, now, rng.random());

    // Here would be the wifi init
    let io = Io::new(peripherals.IO_MUX);

    let analog_pin = peripherals.GPIO26;
    let mut digital_pin = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    // peripherals.DAC2Output::new(io.pins.gpio2, Level::Low);
    let mut dac1 = esp_hal::analog::dac::Dac::new(peripherals.DAC2, analog_pin);
    // let mut dac1 = Dac2::new(peripherals.DAC2, analog_pin);
    let dac1_ref = &mut dac1;
    let reset_pin = Input::new(
        peripherals.GPIO4,
        esp_hal::gpio::InputConfig::default().with_pull(Pull::Down),
    );
    if reset_pin.is_high() {
        handle_device_reset(&mut fs);
    }
    // init bluetooth

    if is_device_configured(&mut fs) {
        let wifi_config = get_wifi_config().unwrap();
        println!("{}", wifi_config.ssid);
        println!("{}", wifi_config.password);
        while !init_wifi(
            &wifi_config.ssid,
            &wifi_config.password,
            &mut controller,
            &stack,
        ) {}
    } else {
        controller.stop().unwrap();

        while !pairing::init_advertising(&hci, &mut controller, &stack) {}
    }
    println!("Start busy loop on main");

    let mut rx_udp_buffer = [0u8; 1536];
    let mut tx_udp_buffer = [0u8; 1536];
    let mut rx_meta = [smoltcp::socket::udp::PacketMetadata::EMPTY; 4];
    let mut tx_meta = [smoltcp::socket::udp::PacketMetadata::EMPTY; 4];
    let mut udp_socket = stack.get_udp_socket(
        rx_meta.as_mut(),
        &mut rx_udp_buffer,
        tx_meta.as_mut(),
        &mut tx_udp_buffer,
    );
    let _msg_id: u16 = 100;
    let _token: u8 = 0;
    //let mut socket = wifi_stack.get_socket(&mut rx_buffer, &mut tx_buffer);
    let socket_port = u16::try_from(rng.random() % 10000).unwrap();
    println!("Port on ESP: {}", socket_port);
    let err = udp_socket.bind(socket_port);
    if err.is_err() {
        println!("IoError");
    }
    let mut coap_client = coap::CoapClient::new(
        udp_socket,
        IpAddress::Ipv4(Ipv4Address::new(
            ip_address[0],
            ip_address[1],
            ip_address[2],
            ip_address[3],
        )),
        port_env,
    );

    let observe_callback = &mut |payload| {
        let payload = String::from_utf8(payload);
        if payload.is_err() {
            // TODO handle errors
            return Err(anyhow!("Invalid payload ( failed to convert from utf8 )"));
        }
        let payload = payload.unwrap();
        let device_state: Result<LightState, serde_json::Error> = serde_json::from_str(&payload);
        if device_state.is_err() {
            return Err(anyhow!("Invalid payload (failed conversion from json)"));
        }
        let device_state = device_state.unwrap();
        if device_state.removed {
            handle_device_reset(&mut fs);
        }
        if device_state.is_on {
            // if cfg!(debug_assertions) {
            // led.set_high();
            // }
            let mut actual_brightness = device_state.brightness;
            actual_brightness /= 5;
            dac1_ref.write(200 + actual_brightness);
            if true {
                digital_pin.set_high();
            }
        } else {
            dac1_ref.write(0);
            digital_pin.set_low();
            // if cfg!(debug_assertions) {
            // led.set_low();
            // }
        }
        println!("{}", payload);
        Ok(())
    };

    loop {
        println!("{}", controller.is_connected().unwrap());
        println!("Making Coap request");
        let _ = coap_client.make_observe_request(
            &format!("lights/{}", device_id),
            true,
            observe_callback,
        );
        match controller.is_connected() {
            Ok(is_connected) => {
                if !is_connected {
                    let _ = controller.connect();
                }
            }
            Err(err) => {
                println!("Error: {:?}", err);
            }
        };
    }
}

//TODO Need to move this to utils
pub fn create_interface(device: &mut esp_wifi::wifi::WifiDevice) -> smoltcp::iface::Interface {
    // users could create multiple instances but since they only have one WifiDevice
    // they probably can't do anything bad with that
    smoltcp::iface::Interface::new(
        smoltcp::iface::Config::new(smoltcp::wire::HardwareAddress::Ethernet(
            smoltcp::wire::EthernetAddress::from_bytes(&device.mac_address()),
        )),
        device,
        timestamp(),
    )
}
fn timestamp() -> smoltcp::time::Instant {
    smoltcp::time::Instant::from_micros(
        esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_micros() as i64,
    )
}
