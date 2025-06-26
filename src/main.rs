#![feature(ascii_char)]
#![feature(error_in_core)]
#![feature(iter_collect_into)]
#![no_std]
#![no_main]
extern crate alloc;

use alloc::format;
use core::mem::MaybeUninit;
use core::str;
use log::LevelFilter;
use utils::{
    get_device_id, get_device_secret, get_wifi_config, handle_device_reset, set_random_mac,
};

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use esp_backtrace as _;

use crate::utils::init_wifi;
use anyhow::anyhow;
use bleps::HciConnector;
use embedded_storage::ReadStorage;
use esp_hal::gpio::{Input, Io, Level, Output};
use esp_hal::prelude::*;
use esp_hal::system::SystemControl;
use esp_hal::{analog::dac::Dac2, clock::ClockControl, peripherals::Peripherals, rng::Rng};
use esp_println::logger::init_logger;
use esp_println::println;
use esp_storage::FlashStorage;
use esp_wifi::{
    ble::controller::BleConnector, current_millis, initialize,
    wifi::utils::create_network_interface, wifi::WifiStaDevice, wifi_interface::WifiStack,
    EspWifiInitFor,
};
use serde::{Deserialize, Serialize};
use smoltcp::{
    iface::SocketStorage,
    wire::{IpAddress, Ipv4Address},
};

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

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();
    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}

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

#[entry]
fn main() -> ! {
    log::set_max_level(LevelFilter::Debug);
    let mut fs = FlashStorage::new();
    let ip_env: &str = core::env!("IP");
    let debug_env: bool = match core::option_env!("DEBUG") {
        Some(val) => val.parse::<bool>().expect("Invalid DEBUG value"),
        None => false,
    };
    println!(core::env!("PORT"));
    println!(core::env!("PORT"));
    // Line currently required for DEVICE_SECRET to appear as a string

    let port_env: u16 = core::env!("PORT")
        .parse::<u16>()
        .expect("PORT is not a valid port");
    init_heap();
    let device_id_bytes = get_device_id(&mut fs);
    let device_id = str::from_utf8(&device_id_bytes).unwrap();
    println!("{}", device_id);

    // Device secret is 344 bytes long
    let device_secret_bytes = get_device_secret(&mut fs);
    // Converting with utf-8 resulted in errors in printable characters
    let device_secret = device_secret_bytes.as_ascii().unwrap().as_str();
    println!("{}", device_secret);
    println!("{:?}", device_secret_bytes);

    init_logger(log::LevelFilter::Info);

    let ip_address = actual_ip(ip_env);

    // Initializing peripherals and clocks
    let peripherals = Peripherals::take();

    let system = SystemControl::new(peripherals.SYSTEM);
    // let mut peripheral_clock_control = system.peripheral_clock_control;
    let clocks = ClockControl::max(system.clock_control).freeze();
    // let mut rtc = Rtc::new(peripherals.RTC_CNTL);
    // rtc.rwdt.disable();

    let timer = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1, &clocks, None).timer0;

    // Initializing wifi
    let mut rng = Rng::new(peripherals.RNG);
    let init = initialize(
        // EspWifiInitFor::WifiBle,
        EspWifiInitFor::Wifi,
        timer,
        rng,
        peripherals.RADIO_CLK,
        &clocks,
    )
    .unwrap();
    set_random_mac(rng).unwrap();

    let wifi = peripherals.WIFI;

    let mut socket_set_entries: [SocketStorage; 3] = Default::default();
    let (iface, device, mut controller, sockets) =
        create_network_interface(&init, wifi, WifiStaDevice, socket_set_entries.as_mut()).unwrap();
    let wifi_stack = WifiStack::new(iface, device, sockets, current_millis);
    let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);

    let analog_pin = io.pins.gpio26;
    let mut digital_pin = Output::new(io.pins.gpio2, Level::Low);
    let mut dac1 = Dac2::new(peripherals.DAC2, analog_pin);
    let dac1_ref = &mut dac1;
    let mut config_bytes = [255u8; 4];
    let reset_pin = Input::new(io.pins.gpio4, esp_hal::gpio::Pull::Down);
    if reset_pin.is_high() {
        handle_device_reset(&mut fs);
    }
    // let reset_pin = Input::new(io.pins.gpio18, esp_hal::gpio::Pull::Down);
    // let reset_level = reset_pin.get_level();
    // println!("{:?}", reset_level);
    // if reset_level == Level::High {
    // 	println!("Resetting the config bytes");
    // 	fs.write(CONFIG_ADDR, &config_bytes).unwrap()
    // } else {
    fs.read(CONFIG_ADDR, &mut config_bytes).unwrap();
    // }
    // if config_bytes == [0, 0, 0, 0] {
    if true {
        let wifi_config = get_wifi_config().unwrap();
        println!("{}", wifi_config.ssid);
        println!("{}", wifi_config.password);
        while !init_wifi(
            &wifi_config.ssid,
            &wifi_config.password,
            &mut controller,
            &wifi_stack,
            &clocks,
        ) {}
    } else {
        let mut bluetooth = peripherals.BT;
        controller.stop().unwrap();
        let connector = BleConnector::new(&init, &mut bluetooth);
        let hci = HciConnector::new(connector, current_millis);
        while !pairing::init_advertising(&hci, &mut controller, &wifi_stack, &clocks) {}
    }
    println!("Start busy loop on main");

    let mut rx_udp_buffer = [0u8; 1536];
    let mut tx_udp_buffer = [0u8; 1536];
    let mut rx_meta = [smoltcp::socket::udp::PacketMetadata::EMPTY; 4];
    let mut tx_meta = [smoltcp::socket::udp::PacketMetadata::EMPTY; 4];
    let mut udp_socket = wifi_stack.get_udp_socket(
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
            if debug_env {
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
