#![feature(ascii_char)]
#![no_std]
#![no_main]
#![no_std]
extern crate alloc;

use alloc::format;
use core::mem::MaybeUninit;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use esp_backtrace as _;

use anyhow::anyhow;
use bleps::{
	ad_structure::{
		create_advertising_data, AdStructure, BR_EDR_NOT_SUPPORTED, LE_GENERAL_DISCOVERABLE,
	},
	att::Uuid,
	Ble, HciConnector,
};
use embedded_io::blocking::Write;
use embedded_storage::{ReadStorage, Storage};
use esp_hal::gpio::{Io, Level, Output};
use esp_hal::system::SystemControl;
use esp_hal::{
	analog::dac::Dac1,
	clock::{ClockControl, CpuClock},
	peripherals::Peripherals,
	prelude::*,
	rng::Rng,
};
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
mod pairing;
mod utils;

const SSID: &str = "HALNy-2.4G-0a3b62_EXT";
// const SSID: &str = "2.4G-dzCr";
// const SSID: &str = "Redmi Note 9 Pro";
const PASSWORD: &str = "$paroladordine";
const DEVICE_ID: &str = "33f808df-e9bf-4001-b364-d129d20993ed";
const FLASH_ADDR: u32 = 0x20000;
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
		.map(|num| {
			let data = match num.to_string().parse::<u8>() {
				Err(_) => {
					panic!("Ip address is wrong");
				}
				Ok(x) => x,
			};
			data
		})
		.collect();
	vec.as_slice().try_into().unwrap()
}

#[entry]
fn main() -> ! {
	let IP: &str = core::env!("IP");
	let is_configured = match core::option_env!("IS_CONFIGURED") {
		Some(val) => val.parse::<bool>().expect("Invalid IS_CONFIGURED value"),
		None => false,
	};
	let DEBUG: bool = match core::option_env!("DEBUG") {
		Some(val) => val.parse::<bool>().expect("Invalid DEBUG value"),
		None => false,
	};
	let port: u16 = core::env!("PORT")
		.parse::<u16>()
		.expect("PORT is not a valid port");
	init_heap();
	let mut fs = FlashStorage::new();
	let mut buf: [u8; 128] = [0u8; 128];
	let mut write_buf: [u8; 5] = [0u8; 5];
	write_buf.copy_from_slice(&b"Hello"[..]);
	// fs.write(FLASH_ADDR,&write_buf);
	// This reads the SSID
	println!("Cap: {}", fs.capacity());
	fs.read(FLASH_ADDR, &mut buf).unwrap();
	println!("{:?}", buf);
	// This reads the password
	fs.read(FLASH_ADDR + 128, &mut buf).unwrap();
	println!("{:?}", buf);
	init_logger(log::LevelFilter::Info);

	let ip_address = actual_ip(IP);

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

	let analog_pin = io.pins.gpio25;
	let mut digital_pin = Output::new(io.pins.gpio2, Level::Low);
	let mut dac1 = Dac1::new(peripherals.DAC1, analog_pin);
	let mut dac1_ref = &mut dac1;
	init_wifi(SSID, PASSWORD, &mut controller, &wifi_stack);
	if !is_configured {
		let mut bluetooth = peripherals.BT;
		controller.stop().unwrap();
		loop {
			let connector = BleConnector::new(&init, &mut bluetooth);
			let hci = HciConnector::new(connector, current_millis);
			pairing::init_advertising(hci, &mut fs);
		}
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
	let mut coapClient = coap::CoapClient::new(
		udp_socket,
		IpAddress::Ipv4(Ipv4Address::new(
			ip_address[0],
			ip_address[1],
			ip_address[2],
			ip_address[3],
		)),
		port,
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
			dac1_ref.write(device_state.brightness);
			if DEBUG {
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
		return Ok(());
	};

	loop {
		println!("{}", controller.is_connected().unwrap());
		println!("Making Coap request");
		coapClient.make_observe_request(&format!("lights/{}", DEVICE_ID), true, observe_callback);
		match controller.is_connected() {
			Ok(is_connected) => {
				if !is_connected {
					controller.connect();
				}
			}
			Err(err) => {
				println!("Error ");
			}
		};
	}
}
