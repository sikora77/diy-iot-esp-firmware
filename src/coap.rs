use alloc::string::String;
use alloc::vec::Vec;
use alloc::{string::ToString, vec};
use anyhow::anyhow;
use coap_lite::{CoapOption, ContentFormat, MessageClass, MessageType, Packet, RequestType};
use embedded_svc::wifi::Wifi;
use esp_println::println;
use esp_wifi::wifi::WifiController;
use esp_wifi::{current_millis, wifi_interface::UdpSocket};
use hal::gpio::{GpioPin, Output, PushPull};
use hal::prelude::_embedded_hal_digital_v2_OutputPin;
use serde::{Deserialize, Serialize};
use smoltcp::wire::IpAddress;

use crate::utils::init_wifi;

#[derive(Serialize, Deserialize, Clone)]
pub struct LightState {
	pub is_on: bool,
	pub brightness: u8,
	pub color: i32,
}

pub struct CoapClient<'a, 'b> {
	pub socket: UdpSocket<'a, 'b>,
	msg_id: u16,
	token: u8,
	ip: IpAddress,
	pin: GpioPin<Output<PushPull>, 2>,
	controller: &'b WifiController<'b>,
}

impl<'a, 'b> CoapClient<'a, 'b> {
	pub fn new(
		socket: UdpSocket<'a, 'b>,
		ip: IpAddress,
		pin: GpioPin<Output<PushPull>, 2>,
		controller: &'b WifiController,
		msg_id: u16,
	) -> Self {
		Self {
			socket: socket,
			msg_id,
			token: 0,
			ip,
			pin,
			controller,
		}
	}
	fn handle_response(&mut self, resp: Packet) {
		let mut packet = Packet::new();
		packet.header.set_type(MessageType::Acknowledgement);
		packet.set_token(resp.get_token().to_vec());
		packet.header.code = MessageClass::Empty;
		packet.header.message_id = resp.header.message_id;
		self.socket.send(self.ip, 5683, &packet.to_bytes().unwrap());
		self.socket.work();
	}
	fn observe(
		&mut self,
		timeout: u64, /* , pin25: GpioPin<Analog, 25>, analog: AvailableAnalog*/
	) {
		println!("Observing");

		//let mut dac1 = dac::DAC1::dac(analog.dac1, pin25).unwrap();

		let mut wait_end = current_millis() + timeout * 1000;
		let mut read_bytes = 0;
		let mut message_bytes: Vec<u8> = vec![];
		loop {
			// println!("{}", ALLOCATOR.free());
			self.socket.work();
			let mut receive_buffer: [u8; 512] = [0; 512];
			//println!("Working the socket");
			let receive_data = self.socket.receive(&mut receive_buffer);
			// Wait to receive entire packet and save it in message_bytes
			if receive_data.is_ok() {
				let receive_data = receive_data.unwrap();
				read_bytes += 512;
				message_bytes.extend_from_slice(&receive_buffer);
				//I have no idea why printing this fixes everything
				println!("{}:{}", read_bytes, receive_data.0);
				if read_bytes > receive_data.0 {
					message_bytes = message_bytes[0..receive_data.0].to_vec();
					let resp = coap_lite::Packet::from_bytes(&message_bytes);
					if resp.is_ok() {
						let resp = resp.unwrap();
						println!("Handling observe");
						let payload = String::from_utf8(resp.clone().payload).unwrap();
						let device_state: LightState = serde_json::from_str(&payload).unwrap();
						//dac1.write(device_state.brightness);

						if device_state.is_on {
							self.pin.set_high().unwrap();
						} else {
							self.pin.set_low().unwrap();
						}
						println!("{}", payload);
						//let device_state: LightState = serde_json::from_str(&payload).unwrap();
						// if (device_state.is_on) {
						// 	let mut led = io.pins.gpio2.into_push_pull_output();

						// 	led.set_high().unwrap();
						// }
						self.handle_response(resp);
						read_bytes = 0;
						message_bytes = Vec::new();
						wait_end = current_millis() + timeout * 1000;
						let is_connected = self.controller.is_connected().unwrap();
						println!("{}", is_connected);
					} else {
						println!("Conversion from bytes to packet failed");
					}
				}
			} else {
				// println!("Nothing to read");
			}
			if current_millis() > wait_end {
				println!("Timeout");
				break;
			}
		}
	}

	// Receive packets
	fn receive(&mut self, timeout: u64) -> Result<coap_lite::Packet, anyhow::Error> {
		let wait_end = current_millis() + timeout * 1000;
		let mut read_bytes = 0;
		let mut message_bytes: Vec<u8> = vec![];
		println!("receiving");

		loop {
			self.socket.work();
			let mut receive_buffer: [u8; 512] = [0; 512];
			let receive_data = self.socket.receive(&mut receive_buffer);
			// Wait to receive entire packet and save it in message_bytes
			if receive_data.is_ok() {
				let receive_data = receive_data.unwrap();
				read_bytes += 512;
				message_bytes.extend_from_slice(&receive_buffer);
				if read_bytes > receive_data.0 {
					message_bytes = message_bytes[0..receive_data.0].to_vec();
					let resp = coap_lite::Packet::from_bytes(&message_bytes);

					if resp.is_ok() {
						return Ok(resp.unwrap());
					}
					return Err(anyhow::Error::msg("Conversion from bytes to packet failed"));
				}
			}
			if current_millis() > wait_end {
				anyhow!("Timeout");
			}
		}
	}
	pub fn make_get_request(
		&mut self,
		uri_path: &str,
		is_confirmable: bool,
	) -> Result<Packet, anyhow::Error> {
		let mut packet = coap_lite::Packet::new();
		match is_confirmable {
			true => {
				packet.header.set_type(MessageType::Confirmable);
			}
			false => packet.header.set_type(MessageType::NonConfirmable),
		}
		packet.set_token(vec![self.token]);
		self.token = self.token.wrapping_add(1);
		packet.header.message_id = self.msg_id;
		self.msg_id = self.msg_id.wrapping_add(1);
		packet.header.code = MessageClass::Request(RequestType::Get);
		packet.set_content_format(ContentFormat::TextPlain);
		uri_path.split("/").for_each(|x| {
			packet.add_option(CoapOption::UriPath, x.to_string().into_bytes());
		});

		let packet_bytes = packet.to_bytes();
		if packet_bytes.is_err() {
			anyhow!("error creating coap packet");
		} else {
			let result = self
				.socket
				.send(self.ip, 5683, &packet.to_bytes().unwrap())
				.is_ok();
			if !result {
				anyhow!("error sending packet");
			}
		}
		println!("Request sent");
		self.socket.work();
		return self.receive(5);
	}
	pub fn make_observe_request(
		&mut self,
		uri_path: &str,
		is_confirmable: bool,
		/*pin25: GpioPin<Analog, 25>,
		analog: AvailableAnalog,*/
	) {
		let mut packet = coap_lite::Packet::new();
		match is_confirmable {
			true => {
				packet.header.set_type(MessageType::Confirmable);
			}
			false => packet.header.set_type(MessageType::NonConfirmable),
		}
		packet.set_token(vec![self.token]);
		//self.token = self.token.wrapping_add(1);
		packet.header.message_id = self.msg_id;
		self.msg_id = self.msg_id.wrapping_add(1);
		packet.header.code = MessageClass::Request(RequestType::Get);
		packet.add_option(CoapOption::Observe, vec![0]);
		packet.set_content_format(ContentFormat::TextPlain);
		uri_path.split("/").for_each(|x| {
			packet.add_option(CoapOption::UriPath, x.to_string().into_bytes());
		});

		let packet_bytes = packet.to_bytes();
		//request.set_path("lights/33f808df-e9bf-4001-b364-d129d20993ed");
		if packet_bytes.is_err() {
			println!("error creating coap packet");
		} else {
			let result = self
				.socket
				.send(self.ip, 5683, &packet.to_bytes().unwrap())
				.is_ok();
			if !result {
				println!("error sending packet");
			}
		}
		let resp = self.receive(5).unwrap();
		println!("Returned: {:?}", resp);
		// self.socket.close();
		// self.socket.bind(6969);
		// loop {
		self.observe(10 /*pin25, analog*/);
		// }
	}
}
