#![feature(ascii_char)]
#![feature(iter_collect_into)]
#![no_std]
#![no_main]
extern crate alloc;

use crate::utils::{get_device_data, get_env, handle_device_reset, init_hardware};
use alloc::format;
use alloc::string::String;
use anyhow::anyhow;
use blocking_network_stack::Stack;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::analog::dac::Dac;
use esp_hal::gpio::{Input, Level, Output, OutputConfig, Pull};
use esp_hal::main;
use esp_hal::peripherals::{DAC2, GPIO2, GPIO26, GPIO4};

use crate::wifi_utils::{
    init_stack_sockets, initialize_network_or_pair, setup_udp_socket, setup_udp_socket_params,
};
use esp_println::println;
use esp_storage::FlashStorage;
use esp_wifi::wifi::WifiController;
use serde::{Deserialize, Serialize};
use utils::now;

esp_bootloader_esp_idf::esp_app_desc!();

mod coap;
mod errors;
mod pairing;
mod utils;
mod wifi_utils;

const CONFIG_ADDR: u32 = 0x9000;
const SSID_ADDR: u32 = 0x9080;
const PASS_ADDR: u32 = 0x9080 + 128;
const ID_ADDR: u32 = 0x9080 + 256;
const SECRET_ADDR: u32 = ID_ADDR + 36;

#[derive(Serialize, Deserialize, Clone)]
pub struct LightState {
    pub is_on: bool,
    pub brightness: u8,
    pub color: i32,
    pub removed: bool,
}

pub struct ESPGpio<'a> {
    pub gpio26_dac: Dac<'a, DAC2<'a>>,
    pub gpio2: Output<'a>,
    pub gpio4: Input<'a>,
}

fn init_gpio<'a>(
    gpio2: GPIO2<'a>,
    dac2: DAC2<'a>,
    gpio26: GPIO26<'a>,
    gpio4: GPIO4<'a>,
) -> ESPGpio<'a> {
    let digital_pin = Output::new(gpio2, Level::Low, OutputConfig::default());
    let dac1 = esp_hal::analog::dac::Dac::new(dac2, gpio26);
    let reset_pin = Input::new(
        gpio4,
        esp_hal::gpio::InputConfig::default().with_pull(Pull::Down),
    );
    ESPGpio {
        gpio26_dac: dac1,
        gpio2: digital_pin,
        gpio4: reset_pin,
    }
}

#[main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();
    // Looks weird but is apparently necessary
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 96 * 1024);
    esp_alloc::heap_allocator!(size: 24 * 1024);

    let (mut rng, hci, mut controller, iface, device, gpio26, gpio2, gpio4, dac2) = init_hardware();
    let mut fs = FlashStorage::new();
    let (port_env, ip_address, debug_env) = get_env();

    let (device_id, _device_secret) = get_device_data(&mut fs);

    let mut socket_set_storage = Default::default();
    let socket_set = init_stack_sockets(&mut socket_set_storage);

    let stack = Stack::new(iface, device, socket_set, now, rng.random());

    let mut gpio_pins = init_gpio(gpio2, dac2, gpio26, gpio4);

    if gpio_pins.gpio4.is_high() {
        handle_device_reset(&mut fs);
    }

    initialize_network_or_pair(&hci, &mut controller, &mut fs, &stack);
    println!("Start busy loop on main");

    let mut wrapper = setup_udp_socket_params();
    let mut udp_socket = setup_udp_socket(&stack, &mut wrapper);

    // Randomize the udp socket port - necessary fo some reason
    let socket_port = u16::try_from(rng.random() % 10000).unwrap() + 1000;
    println!("Port on ESP: {}", socket_port);

    if let Err(_err) = udp_socket.bind(socket_port) {
        println!("IoError ");
    }
    let mut coap_client = coap::CoapClient::new(udp_socket, ip_address, port_env);

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
        if device_state.removed {
            handle_device_reset(&mut fs);
        }
        if device_state.is_on {
            let mut actual_brightness = device_state.brightness;
            actual_brightness /= 5;
            gpio_pins.gpio26_dac.write(200 + actual_brightness);
            if debug_env {
                gpio_pins.gpio2.set_high();
            }
        } else {
            gpio_pins.gpio26_dac.write(0);
            gpio_pins.gpio2.set_low();
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
        reconnect_if_needed(&mut controller);
    }
}

fn reconnect_if_needed(controller: &mut WifiController) {
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
