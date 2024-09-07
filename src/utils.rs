// use embedded_svc::ipv4::Interface;
// use embedded_svc::wifi::{ClientConfiguration, Configuration, Wifi};

use core::str;

use crate::errors::{PasswordFlashError, SSIDFlashError};
use crate::{ID_ADDR, PASS_ADDR, SECRET_ADDR, SSID_ADDR};
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use core::error::Error;
use embedded_storage::ReadStorage;
use esp_backtrace as _;
use esp_println::println;
use esp_storage::FlashStorage;
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiStaDevice};
use esp_wifi::wifi_interface::WifiStack;

const MAX_CONNECTION_TRIES: u8 = 5;

pub struct WifiConfig {
	pub ssid: String,
	pub password: String,
}
pub fn init_wifi(
	ssid: &str,
	password: &str,
	controller: &mut WifiController,
	wifi_stack: &WifiStack<WifiStaDevice>,
) -> bool {
	let client_config = Configuration::Client(ClientConfiguration {
		ssid: ssid.try_into().unwrap(),
		password: password.try_into().unwrap(),
		..Default::default()
	});
	let res = controller.set_configuration(&client_config);
	println!("wifi_set_configuration returned {:?}", res);
	let mut connection_tries = 0;
	controller.start().unwrap();
	println!("is wifi started: {:?}", controller.is_started());
	println!("{:?}", controller.get_capabilities());
	println!("wifi_connect {:?}", controller.connect());

	// wait to get connected
	println!("Wait to get connected");
	loop {
		let res = controller.is_connected();
		match res {
			Ok(connected) => {
				if connected {
					break;
				}
			}
			Err(err) => {
				println!("{:?}", err);
				connection_tries += 1;
				if connection_tries > MAX_CONNECTION_TRIES {
					return false;
				}
			}
		}
	}
	println!("{:?}", controller.is_connected());

	// wait for getting an ip address
	println!("Wait to get an ip address");
	loop {
		wifi_stack.work();

		if wifi_stack.is_iface_up() {
			println!("got ip {:?}", wifi_stack.get_ip_info());
			break;
		}
	}
	true
}
pub fn get_wifi_config() -> Result<WifiConfig, Box<dyn Error>> {
	let mut ssid_buf: [u8; 128] = [0u8; 128];
	let mut fs = FlashStorage::new();
	// This reads the SSID
	fs.read(SSID_ADDR, &mut ssid_buf).unwrap();
	// TODO convert to UTF-8

	let ssid_result = str::from_utf8(&ssid_buf);
	if ssid_result.is_err() {
		return Err(SSIDFlashError.into());
	}
	let mut password_buf: [u8; 128] = [0u8; 128];

	// This reads the password
	fs.read(PASS_ADDR, &mut password_buf).unwrap();
	let pass_result = str::from_utf8(&password_buf);
	if pass_result.is_err() {
		return Err(PasswordFlashError.into());
	}
	Ok(WifiConfig {
		ssid: ssid_result.unwrap().trim_matches(char::from(0)).to_owned(),
		password: pass_result.unwrap().trim_matches(char::from(0)).to_owned(),
	})
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
