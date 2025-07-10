use crate::errors::{PasswordFlashError, SSIDFlashError};
use crate::utils::is_device_configured;
use crate::{pairing, PASS_ADDR, SSID_ADDR};
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use bleps::HciConnector;
use blocking_network_stack::{Stack, UdpSocket};
use core::error::Error;
use embedded_storage::ReadStorage;
use esp_println::println;
use esp_storage::FlashStorage;
use esp_wifi::ble::controller::BleConnector;
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice};
use smoltcp::iface::{SocketSet, SocketStorage};
use smoltcp::socket::udp::PacketMetadata;
use smoltcp::wire::DhcpOption;

const MAX_CONNECTION_TRIES: u8 = 5;
#[derive(Copy, Clone)]
#[allow(clippy::upper_case_acronyms)]
enum WifiFieldType {
    SSID = SSID_ADDR as isize,
    Password = PASS_ADDR as isize,
}
fn read_wifi_field_from_flash(
    fs: &mut FlashStorage,
    field_type: WifiFieldType,
) -> Result<String, Box<dyn Error>> {
    let mut ssid_buf: [u8; 128] = [0u8; 128];
    fs.read(field_type as u32, &mut ssid_buf).unwrap();

    let ssid_result = str::from_utf8(&ssid_buf);
    if ssid_result.is_err() {
        return match field_type {
            WifiFieldType::SSID => Err(SSIDFlashError.into()),
            WifiFieldType::Password => Err(PasswordFlashError.into()),
        };
    }
    Ok(ssid_result.unwrap().trim_matches(char::from(0)).to_owned())
}
pub fn get_wifi_config() -> Result<Configuration, Box<dyn Error>> {
    let mut fs = FlashStorage::new();
    let ssid = read_wifi_field_from_flash(&mut fs, WifiFieldType::SSID)?;
    let password = read_wifi_field_from_flash(&mut fs, WifiFieldType::Password)?;

    println!("Wifi config:");
    println!("SSID: {}", ssid);
    println!("Password: {}", password);

    Ok(Configuration::Client(ClientConfiguration {
        ssid,
        password,
        ..Default::default()
    }))
}
pub fn connect_to_wifi(controller: &mut WifiController, wifi_stack: &Stack<WifiDevice>) -> bool {
    match get_wifi_config() {
        Ok(config) => try_connect_to_network(&config, controller, wifi_stack),
        Err(_) => false,
    }
}
pub fn try_connect_to_network(
    client_config: &Configuration,
    controller: &mut WifiController,
    wifi_stack: &Stack<WifiDevice>,
) -> bool {
    let res = controller.set_configuration(client_config);
    println!("wifi_set_configuration returned {:?}", res);
    let mut connection_tries = 0;
    controller.start().unwrap();
    println!("is wifi started: {:?}", controller.is_started());
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

pub struct UdpSocketParamsWrapper {
    rx_udp_buffer: [u8; 1536],
    tx_udp_buffer: [u8; 1536],
    rx_meta: [PacketMetadata; 4],
    tx_meta: [PacketMetadata; 4],
}
pub fn setup_udp_socket_params() -> UdpSocketParamsWrapper {
    UdpSocketParamsWrapper {
        rx_udp_buffer: [0u8; 1536],
        tx_udp_buffer: [0u8; 1536],
        rx_meta: [PacketMetadata::EMPTY; 4],
        tx_meta: [PacketMetadata::EMPTY; 4],
    }
}
pub fn setup_udp_socket<'a>(
    stack: &'a Stack<'a, WifiDevice<'a>>,
    wrapper: &'a mut UdpSocketParamsWrapper,
) -> UdpSocket<'a, 'a, WifiDevice<'a>> {
    stack.get_udp_socket(
        &mut wrapper.rx_meta,
        &mut wrapper.rx_udp_buffer,
        &mut wrapper.tx_meta,
        &mut wrapper.tx_udp_buffer,
    )
}

pub fn initialize_network_or_pair(
    hci: &HciConnector<BleConnector>,
    controller: &mut WifiController,
    fs: &mut FlashStorage,
    stack: &Stack<WifiDevice>,
) {
    if is_device_configured(fs) {
        while !connect_to_wifi(controller, stack) {}
    } else {
        controller.stop().unwrap();
        while !pairing::init_advertising(hci, controller, stack) {}
    }
}

pub fn init_stack_sockets<'a>(socket_set_entries: &'a mut [SocketStorage<'a>; 3]) -> SocketSet<'a> {
    let mut socket_set = SocketSet::new(&mut socket_set_entries[..]);
    let mut dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
    // we can set a hostname here (or add other DHCP options)
    dhcp_socket.set_outgoing_options(&[DhcpOption {
        kind: 12,
        data: b"esp-wifi",
    }]);
    socket_set.add(dhcp_socket);
    socket_set
}
