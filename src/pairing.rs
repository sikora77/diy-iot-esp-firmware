use bleps::{
	ad_structure::{
		create_advertising_data, AdStructure, BR_EDR_NOT_SUPPORTED, LE_GENERAL_DISCOVERABLE,
	},
	attribute_server::{AttributeServer, WorkResult},
	gatt, Ble, HciConnector,
};
use embedded_io::blocking::Write;
use esp_storage::FlashStorage;

use esp_println::println;
use esp_wifi::ble::controller::BleConnector;

#[allow(non_snake_case)]
pub fn init_advertising(hci: HciConnector<BleConnector>, mut _fs: &mut FlashStorage) {
	println!("Begin bluetooth stuff");
	let mut ble = Ble::new(&hci);
	ble.init().unwrap();
	ble.cmd_set_le_advertising_parameters().unwrap();
	ble.cmd_set_le_advertising_data(
		create_advertising_data(&[
			AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
			AdStructure::ServiceUuids16(&[Uuid::Uuid16(0x1809)]),
			AdStructure::CompleteLocalName("Fancy lights"),
		])
		.unwrap(),
	)
	.unwrap();
	ble.cmd_set_le_advertise_enable(true).unwrap();
	println!("Started advertising");

	let mut read_id = |offset: usize, mut data: &mut [u8]| {
		let id_bytes = crate::DEVICE_ID.as_bytes();
		// Need to write from offset to end, sometimes we can't transmit the entire message
		data.write(&id_bytes[offset..]).unwrap();
		crate::DEVICE_ID.len() - offset
	};
	let mut write_wifi_ssid = |offset: usize, data: &[u8]| {
		println!(
			"RECEIVED SSID: {} {:?}",
			offset,
			data.as_ascii().map(|str_data| str_data.as_str())
		);
	};

	let mut write_wifi_password = |offset: usize, data: &[u8]| {
		println!(
			"RECEIVED PASSWORD: {} {:?}",
			offset,
			data.as_ascii().map(|str_data| str_data.as_str())
		);
	};

	let mut read_secret = |offset: usize, mut data: &mut [u8]| {
		let hello = &b"Hola!"[..];
		data.write(hello).unwrap();
		30 - offset
	};
	gatt!([service {
		uuid: "937312e0-2354-11eb-9f10-fbc30a62cf38",
		characteristics: [
			characteristic {
				name: "Device_Id",
				uuid: "2137",
				read: read_id,
			},
			characteristic {
				name: "Device_Secret",
				uuid: "987312e0-2354-11eb-9f10-fbc30a62cf38",
				read: read_secret,
			},
			characteristic {
				uuid: "937312e0-2354-11eb-9f10-fbc30a62cf39",
				name: "WiFi_SSID",
				write: write_wifi_ssid,
			},
			characteristic {
				name: "WiFi_Password",
				uuid: "987312e0-2354-11eb-9f10-fbc30a62cf40",
				write: write_wifi_password,
			},
		],
	},]);

	let mut rng = bleps::no_rng::NoRng;
	let mut srv = AttributeServer::new(&mut ble, &mut gatt_attributes, &mut rng);

	loop {
		match srv.do_work() {
			Ok(x) => {
				if x == WorkResult::GotDisconnected {
					break;
				}
			}
			Err(e) => {
				println!("{:?}", e);
			}
		};
	}
}
