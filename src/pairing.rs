use core::cmp::min;
use bleps::{
    ad_structure::{
        create_advertising_data,
        AdStructure,
        BR_EDR_NOT_SUPPORTED,
        LE_GENERAL_DISCOVERABLE,
    },
    attribute_server::{AttributeServer, NotificationData, WorkResult},
    gatt,
    Ble,
    HciConnector,
};
use bleps::att::Uuid;
use embedded_storage::{ReadStorage, Storage};
use esp_storage::FlashStorage;
use embedded_io::blocking::Write;

use esp_println::{print, println};
use esp_wifi::ble::controller::BleConnector;
use crate::FLASH_ADDR;

pub fn init_advertising(mut hci: HciConnector<BleConnector>, mut fs: &mut FlashStorage) {
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
            .unwrap()
    ).unwrap();
    ble.cmd_set_le_advertise_enable(true).unwrap();
    println!("Started advertising");
    let mut read_id = |_offset: usize, mut data: &mut [u8]| {
        println!("{}", crate::DEVICE_ID.len());
        let id_bytes = crate::DEVICE_ID.as_bytes();
        data.write(&id_bytes).unwrap();
        crate::DEVICE_ID.len()
    };
    let write_wifi_ssid = |offset: usize, data: &[u8]| {
        println!("RECEIVED SSID: {} {:?}", offset, match data.as_ascii() {
            Some(str_data) => Some(str_data.as_str()),
            None => None
        });
        // let data_len = min(data.len(), 128);
        // println!("Not done");
        // match fs.write(FLASH_ADDR, &data[0..data_len]) {
        //     Ok(_) => { println!("Write ok"); }
        //     Err(e) => { println!("{:?}", e); }
        // };
        // println!("Wrote data");
        // let mut buf: [u8; 128] = [0u8; 128];
        // // This reads the SSID
        // fs.read(FLASH_ADDR, &mut buf).unwrap();
        // println!("{:?}", buf);
        // // This reads the password
        // fs.read(FLASH_ADDR+128, &mut buf).unwrap();
        // println!("{:?}", buf);
    };

    let write_wifi_password = |offset: usize, data: &[u8]| {
        println!("RECEIVED PASSWORD: {} {:?}", offset, match data.as_ascii() {
            Some(str_data) => Some(str_data.as_str()),
            None => None
        });
        // let mut fs = FlashStorage::new();
        // let data_len = min(data.len(), 128);
        // fs.write(128, &data[0..data_len]).unwrap();
        // let mut buf: [u8; 128] = [0u8; 128];
        // // This reads the SSID
        // fs.read(0, &mut buf).unwrap();
        // println!("{:?}", buf);
        // // This reads the password
        // fs.read(128, &mut buf).unwrap();
        // println!("{:?}", buf);
    };

    let mut read_secret = |_offset: usize, mut data: &mut [u8]| {
        let hello = &b"Hola!"[..];
        data.write(hello).unwrap();
        30
    };
    let mut wf3 = |offset: usize, data: &[u8]| {
        println!("RECEIVED: Offset {}, data {:?}", offset, data);
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
        let mut cccd = [0u8; 1];
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