use alloc::vec::Vec;
use alloc::{string::ToString, vec};
use anyhow::{anyhow, Error};
use blocking_network_stack::UdpSocket;
use coap_lite::{CoapOption, ContentFormat, MessageClass, MessageType, Packet, RequestType};
use esp_println::println;
use esp_wifi::wifi::WifiDevice;
use log::{log, Level};
use smoltcp::wire::IpAddress;

use crate::utils::now;

pub struct CoapClient<'a, 'b> {
    pub socket: UdpSocket<'a, 'b, WifiDevice<'a>>,
    msg_id: u16,
    token: u8,
    ip: IpAddress,
    port: u16,
}

impl<'a, 'b> CoapClient<'a, 'b> {
    pub fn new(socket: UdpSocket<'a, 'b, WifiDevice<'a>>, ip: IpAddress, port: u16) -> Self {
        Self {
            socket,
            msg_id: 0,
            token: 0,
            ip,
            port,
        }
    }
    fn handle_acknowledgement(&mut self, resp: Packet) {
        let mut packet = Packet::new();
        packet.header.set_type(MessageType::Acknowledgement);
        packet.set_token(resp.get_token().to_vec());
        packet.header.code = MessageClass::Empty;
        packet.header.message_id = resp.header.message_id;
        let _ = self
            .socket
            .send(self.ip, self.port, &packet.to_bytes().unwrap());
    }

    // Receive packets
    fn receive(&mut self, timeout: u64) -> Result<coap_lite::Packet, anyhow::Error> {
        let wait_end = now() + timeout * 1000;
        let mut read_bytes = 0;
        let mut message_bytes: Vec<u8> = vec![];
        self.socket.work();
        log!(Level::Debug, "Receiving");
        loop {
            let mut receive_buffer: [u8; 512] = [0; 512];
            let receive_data = self.socket.receive(&mut receive_buffer);

            // Wait to receive entire packet and save it in message_bytes
            if receive_data.is_err() {
                return Err(anyhow!("UDP error"));
            }
            let receive_data = receive_data.unwrap();
            read_bytes += 512;
            message_bytes.extend_from_slice(&receive_buffer);
            if read_bytes > receive_data.0 {
                message_bytes = message_bytes[0..receive_data.0].to_vec();
                return match Packet::from_bytes(&message_bytes) {
                    Ok(resp) => Ok(resp),
                    Err(_) => Err(anyhow::Error::msg("Conversion from bytes to packet failed")),
                };
            }
            Self::check_timeout(wait_end)?;
        }
    }

    fn check_timeout(wait_end: u64) -> Result<(), Error> {
        if now() > wait_end {
            println!("Timeout");
            return Err(anyhow!("Timeout"));
        }
        Ok(())
    }

    fn create_get_packet(
        &mut self,
        uri_path: &str,
        is_confirmable: bool,
        add_to_token: bool,
        observable: bool,
    ) -> Packet {
        let mut packet = coap_lite::Packet::new();
        match is_confirmable {
            true => {
                packet.header.set_type(MessageType::Confirmable);
            }
            false => packet.header.set_type(MessageType::NonConfirmable),
        }
        packet.set_token(vec![self.token]);
        if add_to_token {
            self.token = self.token.wrapping_add(1);
        }
        packet.header.message_id = self.msg_id;
        self.msg_id = self.msg_id.wrapping_add(1);
        packet.header.code = MessageClass::Request(RequestType::Get);
        if observable {
            packet.add_option(CoapOption::Observe, vec![0]);
        }
        packet.set_content_format(ContentFormat::TextPlain);
        uri_path.split('/').for_each(|x| {
            packet.add_option(CoapOption::UriPath, x.to_string().into_bytes());
        });
        packet
    }
    pub fn make_get_request(
        &mut self,
        uri_path: &str,
        is_confirmable: bool,
        add_to_token: bool,
        observable: bool,
    ) -> Result<Packet, anyhow::Error> {
        let packet = self.create_get_packet(uri_path, is_confirmable, add_to_token, observable);

        let packet_bytes = packet.to_bytes();
        if packet_bytes.is_err() {
            return Err(anyhow!("error creating coap packet"));
        } else {
            let result = self
                .socket
                .send(self.ip, self.port, &packet.to_bytes().unwrap())
                .is_ok();
            if !result {
                return Err(anyhow!("error sending packet"));
            }
        }
        println!("Request sent");
        self.socket.work();
        self.msg_id += 1;
        self.receive(5)
    }

    pub fn make_observe_request<F: FnMut(Vec<u8>) -> Result<(), anyhow::Error>>(
        &mut self,
        uri_path: &str,
        is_confirmable: bool,
        response_callback: &mut F,
    ) -> Result<(), anyhow::Error> {
        let resp = self.make_get_request(uri_path, is_confirmable, true, true);
        if resp.is_err() {
            log!(Level::Debug, "{}", resp.unwrap_err());
            // println!("{:?}", resp.unwrap_err());
        }
        self.observe(10, response_callback)
    }

    fn observe<F: FnMut(Vec<u8>) -> Result<(), anyhow::Error>>(
        &mut self,
        timeout: u64,
        mut response_callback: F,
    ) -> Result<(), anyhow::Error> {
        println!("Observing");
        let mut wait_end = now() + timeout * 1000;
        loop {
            let resp = self.receive(timeout);
            if resp.is_ok() {
                let resp = resp.unwrap();
                println!("Handling observe");
                response_callback(resp.payload.clone())?;
                self.handle_acknowledgement(resp);
                self.msg_id += 1;

                wait_end = now() + timeout * 1000;
            } else {
                log!(Level::Debug, "{}", resp.unwrap_err());
            }
            Self::check_timeout(wait_end)?;
        }
    }
}
