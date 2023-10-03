#![no_std]
#![no_main]
extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use embedded_svc::wifi::Wifi;
use esp_backtrace as _;
use esp_println::logger::init_logger;
use esp_println::println;
use esp_wifi::wifi::utils::create_network_interface;
use esp_wifi::wifi::WifiMode;
use esp_wifi::wifi_interface::WifiStack;
use esp_wifi::{current_millis, initialize, EspWifiInitFor};
use hal::clock::{ClockControl, CpuClock};
use hal::gpio::IO;
use hal::timer::TimerGroup;
use hal::Rng;
use hal::{peripherals::Peripherals, prelude::*, Rtc};
use smoltcp::iface::SocketStorage;
use smoltcp::socket::udp::PacketMetadata;
use smoltcp::wire::{IpAddress, Ipv4Address};

use crate::utils::init_wifi;

mod coap;
mod utils;

const SSID: &str = "HALNy-2.4G-0a3b62";
const PASSWORD: &str = "$paroladordine";
const IP: &str = core::env!("IP");

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
	const HEAP_SIZE: usize = 2 * 1024;

	extern "C" {
		static mut _heap_start: u32;
	}
	unsafe {
		let heap_start = &_heap_start as *const _ as usize;
		ALLOCATOR.init(heap_start as *mut u8, HEAP_SIZE);
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
			return data;
		})
		.collect();
	vec.as_slice().try_into().unwrap()
}

#[entry]
fn main() -> ! {
	init_heap();
	init_logger(log::LevelFilter::Info);

	let ip_address = actual_ip(IP);

	// Initializing peripherals and clocks
	let peripherals = Peripherals::take();

	let system = peripherals.DPORT.split();
	let mut peripheral_clock_control = system.peripheral_clock_control;
	let clocks = ClockControl::configure(system.clock_control, CpuClock::Clock240MHz).freeze();
	let mut rtc = Rtc::new(peripherals.RTC_CNTL);
	rtc.rwdt.disable();

	let timer = TimerGroup::new(peripherals.TIMG1, &clocks, &mut peripheral_clock_control).timer0;

	// Initializing wifi
	let mut rng = Rng::new(peripherals.RNG);
	let init = initialize(
		EspWifiInitFor::Wifi,
		timer,
		rng,
		system.radio_clock_control,
		&clocks,
	)
	.unwrap();

	let (wifi, _) = peripherals.RADIO.split();
	let mut socket_set_entries: [SocketStorage; 3] = Default::default();
	let (iface, device, mut controller, sockets) =
		create_network_interface(&init, wifi, WifiMode::Sta, &mut socket_set_entries).unwrap();
	let wifi_stack = WifiStack::new(iface, device, sockets, current_millis);
	let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);
	let analog = peripherals.SENS.split();
	let pin25 = io.pins.gpio25.into_analog();
	let mut led = io.pins.gpio2.into_push_pull_output();
	init_wifi(SSID, PASSWORD, &mut controller, &wifi_stack);

	println!("Start busy loop on main");

	let mut rx_udp_buffer = [0u8; 1536];
	let mut tx_udp_buffer = [0u8; 1536];
	let mut rx_meta = [PacketMetadata::EMPTY];
	let mut tx_meta = [PacketMetadata::EMPTY];
	let mut udp_socket = wifi_stack.get_udp_socket(
		&mut rx_meta,
		&mut rx_udp_buffer,
		&mut tx_meta,
		&mut tx_udp_buffer,
	);
	let mut msg_id: u16 = 100;
	let mut token: u8 = 0;
	//let mut socket = wifi_stack.get_socket(&mut rx_buffer, &mut tx_buffer);
	let err = udp_socket.bind(6969);
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
		led,
		&controller,
		rng.random() as u16,
	);

	coapClient.make_observe_request(
		"lights/33f808df-e9bf-4001-b364-d129d20993ed",
		true,
		// pin25,
		// analog,
	);
	let mut read_bytes = 0;
	let mut message_bytes: Vec<u8> = Vec::new();
	// We never get to this point
	loop {
		println!("Making Coap request");
		coapClient.make_observe_request(
			"lights/33f808df-e9bf-4001-b364-d129d20993ed",
			true,
			// 	pin25,
			// 	analog,
		);
		println!("{}", controller.is_connected().unwrap());
		coapClient.socket.work();

		let mut receive_buffer: [u8; 64] = [0; 64];
		match coapClient.socket.receive(&mut receive_buffer) {
			Ok(x) => {
				read_bytes += 64;
				message_bytes.extend_from_slice(&receive_buffer);
				if read_bytes >= x.0 {
					let resp = coap_lite::Packet::from_bytes(&message_bytes);
					println!("{:?}", String::from_utf8(resp.unwrap().payload));
				}
			}
			Err(err) => {
				println!("error");
			}
		}

		let wait_end = current_millis() + 5 * 1000;
		while current_millis() < wait_end {
			coapClient.socket.work();
		}
	}
}
