#![feature(ascii_char)]
#![feature(error_in_core)]
#![feature(iter_collect_into)]
#![no_std]
#![no_main]
extern crate alloc;

use alloc::format;
use core::mem::MaybeUninit;
use core::str;
use embedded_storage::nor_flash::NorFlash;
use utils::get_wifi_config;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use esp_backtrace as _;

use anyhow::anyhow;
use bleps::HciConnector;
use embedded_storage::ReadStorage;
use esp_hal::gpio::{Io, Level, Output};
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

use crate::utils::init_wifi;

mod coap;
mod errors;
mod pairing;
mod utils;

// const SSID: &str = "HALNy-2.4G-0a3b62_EXT";
// const SSID: &str = "NETIASPOT-asgndF5-2.4G";
// const SSID: &str = "2.4G-dzCr";
// const SSID: &str = "Redmi Note 9 Pro";
// const PASSWORD: &str = "$paroladordine";
// const PASSWORD: &str = "4vuDJn3eDEHvw3st8w";
// const DEVICE_ID: &str = "33f808df-e9bf-4001-b364-d129d20993ed";
const DEVICE_ID: &str = "EXAMPLE1-DEVI-CEID-DEV1-SAMPLEDEVICE";
const DEVICE_SECRET: &str = "LONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRETLONGDEVICESECRET8letters";
const CONFIG_ADDR: u32 = 0x9000;
const SSID_ADDR: u32 = 0x9080;
const PASS_ADDR: u32 = 0x9080 + 128;
const ID_ADDR: u32 = 0x9080 + 256;
const SECRET_ADDR: u32 = 0x9080 + 320;
// const PASSWORD: &str = "bVztpcdj";
// const PASSWORD: &str = "serwis15";

#[derive(Serialize, Deserialize, Clone)]
pub struct LightState {
	pub is_on: bool,
	pub brightness: u8,
	pub color: i32,
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
	let ip_env: &str = core::env!("IP");
	let is_configured_env = match core::option_env!("IS_CONFIGURED") {
		Some(val) => val.parse::<bool>().expect("Invalid IS_CONFIGURED value"),
		None => false,
	};
	let debug_env: bool = match core::option_env!("DEBUG") {
		Some(val) => val.parse::<bool>().expect("Invalid DEBUG value"),
		None => false,
	};
	println!(core::env!("PORT"));
	// Line currently required for DEVICE_SECRET to appear as a string
	println!("{}", DEVICE_SECRET);

	let port_env: u16 = core::env!("PORT")
		.parse::<u16>()
		.expect("PORT is not a valid port");
	init_heap();
	let mut fs = FlashStorage::new();
	let mut buf: [u8; 128] = [0u8; 128];
	// Device ID is 36 bytes long
	fs.read(ID_ADDR, &mut buf).unwrap();
	let device_id = str::from_utf8(&buf[0..36]).unwrap();
	// Device secret is 344 bytes long
	let mut secret_buf: [u8; 512] = [0u8; 512];
	fs.read(SECRET_ADDR, &mut secret_buf).unwrap();

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
		EspWifiInitFor::WifiBle,
		timer,
		rng,
		peripherals.RADIO_CLK,
		&clocks,
	)
	.unwrap();

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
	fs.read(CONFIG_ADDR, &mut config_bytes).unwrap();
	if config_bytes == [0, 0, 0, 0] {
		let wifi_config = get_wifi_config().unwrap();
		while !init_wifi(
			&wifi_config.ssid,
			&wifi_config.password,
			&mut controller,
			&wifi_stack,
		) {}
	} else {
		// let wifi_config_result = get_wifi_config();
		// let mut is_wifi_configured = true;
		// if wifi_config_result.is_err() {
		// 	is_wifi_configured = false;
		// }

		// if is_wifi_configured {
		// 	let wifi_config = wifi_config_result.unwrap();
		// 	println!("Wifi config:");
		// 	println!("SSID: {}", wifi_config.ssid);
		// 	println!("Password: {}", wifi_config.password);
		// 	if !init_wifi(
		// 		&wifi_config.ssid,
		// 		&wifi_config.password,
		// 		&mut controller,
		// 		&wifi_stack,
		// 	) {
		// 		is_wifi_configured = false;
		// 	}
		// }
		// if !is_configured_env || !is_wifi_configured {
		let mut bluetooth = peripherals.BT;
		controller.stop().unwrap();
		loop {
			let connector = BleConnector::new(&init, &mut bluetooth);
			let hci = HciConnector::new(connector, current_millis);
			if pairing::init_advertising(hci) {
				let wifi_config_result = get_wifi_config();
				let mut is_wifi_configured = true;
				if wifi_config_result.is_err() {
					is_wifi_configured = false;
				}

				if is_wifi_configured {
					let wifi_config = wifi_config_result.unwrap();
					println!("Wifi config:");
					println!("SSID: {}", wifi_config.ssid);
					println!("Password: {}", wifi_config.password);
					if init_wifi(
						&wifi_config.ssid,
						&wifi_config.password,
						&mut controller,
						&wifi_stack,
					) {
						let config_bytes = [0u8; 4];
						fs.write(CONFIG_ADDR, &config_bytes).unwrap();
						break;
					}
				}
			}
		}
		// }
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
