use alloc::vec;
use alloc::vec::Vec;
use bleps::{
	ad_structure::{
		create_advertising_data, AdStructure, BR_EDR_NOT_SUPPORTED, LE_GENERAL_DISCOVERABLE,
	},
	attribute_server::{AttributeServer, WorkResult},
	gatt, Ble, HciConnector,
};
use embedded_io::blocking::Write;
use embedded_storage::Storage;
use esp_backtrace as _;
use esp_println::println;
use esp_storage::FlashStorage;
use esp_wifi::ble::controller::BleConnector;

use crate::{PASS_ADDR, SSID_ADDR};

#[allow(non_snake_case)]
pub fn init_advertising(hci: HciConnector<BleConnector>) {
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
	let mut ssid_buf: [u8; 128] = [0u8; 128];
	let ssid_offset: u8 = 0;
	let mut ssid_message_started = false;
	let mut write_wifi_ssid = |_offset: usize, data: &[u8]| {
		// let mut ssid_data: Vec<u8> = vec![];
		// data.iter().collect_into(&mut ssid_data);
		// //TODO For current debug I have to change this
		// // also debug, message start is 4 dots
		// println!("{:?}", ssid_data);
		// // Recognize start of message
		// if ssid_data.len() > 4 && ssid_data[0..4] == [46, 46, 46, 46] && !ssid_message_started {
		// 	// This is a start of a message
		// 	ssid_message_started = true;
		// 	ssid_offset = 0;
		// 	ssid_data.drain(0..4);
		// }
		// if !ssid_message_started {
		// 	// Welp, something went horribly wrong
		// 	println!("The horribly wrong happened")
		// } else {
		// 	// Recognize end of message
		// 	if ssid_data[ssid_data.len() - 4..ssid_data.len()] == [46, 46, 46, 46] {
		// 		ssid_message_started = false;
		// 		ssid_buf[ssid_offset as usize..(ssid_offset as usize + ssid_data.len() - 4)]
		// 			.copy_from_slice(&ssid_data[0..ssid_data.len() - 4]);
		// 		let data_size = ((ssid_offset + 20) / 8) * 8;
		// 		println!(
		// 			"RECEIVED SSID: {} {:?}",
		// 			offset,
		// 			&ssid_buf[0..data_size as usize]
		// 				.as_ascii()
		// 				.map(|str_data| str_data.as_str())
		// 		);
		// 		fs.write(SSID_ADDR, &ssid_buf).unwrap();
		// 		let mut read_bytes: [u8; 128] = [0u8; 128];
		// 		fs.read(SSID_ADDR, &mut read_bytes).unwrap();
		// 		println!("READ FROM FLASH: {:?}", read_bytes);
		// 		return;
		// 	}
		// 	ssid_buf[ssid_offset as usize..(ssid_offset as usize + ssid_data.len())]
		// 		.copy_from_slice(&ssid_data);
		// 	ssid_offset += ssid_data.len() as u8;
		// }
		handle_write(
			&mut ssid_buf,
			&mut ssid_message_started,
			SSID_ADDR,
			&mut (ssid_offset as usize),
			data,
		)
	};
	let mut pass_buf: [u8; 128] = [0u8; 128];
	let mut pass_offset: u8 = 0;
	let mut pass_message_started = false;
	let mut write_wifi_password = |_offset: usize, data: &[u8]| {
		handle_write(
			&mut pass_buf,
			&mut pass_message_started,
			PASS_ADDR,
			&mut (pass_offset as usize),
			data,
		)
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

fn handle_write(
	buf: &mut [u8],
	message_started: &mut bool,
	address: u32,
	offset: &mut usize,
	data: &[u8],
) {
	let mut fs = FlashStorage::new();
	let mut write_data: Vec<u8> = vec![];
	data.iter().collect_into(&mut write_data);
	//TODO For current debug I have to change this
	// also debug, message start is 4 dots
	println!("{:?}", write_data);
	// Recognize start of message
	if write_data.len() > 4 && write_data[0..4] == [46, 46, 46, 46] && !*message_started {
		// This is a start of a message
		*message_started = true;
		*offset = 0;
		write_data.drain(0..4);
	}
	if !*message_started {
		// Welp, something went horribly wrong
		println!("The horribly wrong happened")
	} else {
		// Recognize end of message
		if write_data[write_data.len() - 4..write_data.len()] == [46, 46, 46, 46] {
			*message_started = false;
			buf[*offset..(*offset + write_data.len() - 4)]
				.copy_from_slice(&write_data[0..write_data.len() - 4]);
			let data_size = ((*offset + 20) / 8) * 8;
			// println!(
			// 	"RECEIVED SSID: {} {:?}",
			// 	offset,
			// 	&buf[0..data_size as usize]
			// 		.as_ascii()
			// 		.map(|str_data| str_data.as_str())
			// );
			fs.write(address, &buf).unwrap();
			let mut read_bytes: [u8; 128] = [0u8; 128];
			// fs.read(SSID_ADDR, &mut read_bytes).unwrap();
			// println!("READ FROM FLASH: {:?}", read_bytes);
			return;
		}
		buf[*offset..(*offset + write_data.len())].copy_from_slice(&write_data);
		*offset += write_data.len();
	}
}
