// use embedded_svc::ipv4::Interface;
// use embedded_svc::wifi::{ClientConfiguration, Configuration, Wifi};

use esp_backtrace as _;
use esp_println::println;
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiStaDevice};
use esp_wifi::wifi_interface::WifiStack;

pub fn init_wifi(
	ssid: &str,
	password: &str,
	controller: &mut WifiController,
	wifi_stack: &WifiStack<WifiStaDevice>,
) {
	let client_config = Configuration::Client(ClientConfiguration {
		ssid: ssid.try_into().unwrap(),
		password: password.try_into().unwrap(),
		..Default::default()
	});
	let res = controller.set_configuration(&client_config);
	println!("wifi_set_configuration returned {:?}", res);

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
}
