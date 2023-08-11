use std::{
    collections::HashMap,
    net::{SocketAddr, TcpListener as StdTcpListener},
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    },
};
use crate::error::Error;
use parking_lot::Mutex;
use once_cell::sync::OnceCell;
use bytes::BufMut;
use tokio::io::{AsyncRead, AsyncReadExt};
use bytes::Bytes;


pub use netstack_lwip as netstack;

pub static UDP_SESSIONS: OnceCell<Mutex<HashMap<u16, UdpSession>>> = OnceCell::new();
//pub static UDP_SOCKET: OnceCell<Arc<Box<netstack::UdpSocket>>> = OnceCell::new();
#[derive(Clone)]
pub struct UdpSession {
    pub assoc_id: u16,
    pub port: u16,
}

impl UdpSession {
    pub fn new(
        assoc_id: u16,
        port: u16,
    ) -> Result<Self, Error> { 
        Ok(Self {
            assoc_id,
            port,
        })
    }

    pub async fn send(&self, pkt: Bytes, src_addr: SocketAddr) -> Result<(), Error> {
        // let src_addr_display = src_addr.to_string();

        // if let Err(err) = tx.send_to(pkt, 0, src_addr).await {
        //     return Err(Error::Io(err));
        // }

        Ok(())
    }
}