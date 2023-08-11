use crate::{config::Local, error::Error};
use crate::connection::{Connection as TuicConnection, ERROR_CODE};
use once_cell::sync::OnceCell;

use parking_lot::Mutex;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tun::{Configuration, r#async::AsyncDevice, TunPacket};
use tokio::io::{self, AsyncWriteExt};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use futures::{SinkExt, StreamExt, Future};
use tuic::Address as TuicAddress;
use tuic_quinn::{Connect, Packet};
use bytes::Bytes;
use std::{
    collections::HashMap,
    io::{Error as IoError, ErrorKind},
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    },
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use lazy_static::lazy_static;

pub mod udp_session;
pub mod handle_task;

pub use netstack_lwip as netstack;

pub struct TapProxy {
    net_stack: netstack::NetStack,
    tcp_listener: netstack::TcpListener,
    udp_socket: Box<netstack::UdpSocket>,
    dev: AsyncDevice,
    next_assoc_id: AtomicU16,
}

impl TapProxy {
    pub fn new() -> Result<Self, Error> {
        let mut config = Configuration::default();
        config
            .name("vtest")
            .address("240.0.0.1")
            .netmask("255.255.255.0")
            .up(); 
        let dev = tun::create_as_async(&config).unwrap();
        let (net_stack, mut tcp_listener, udp_socket) = netstack::NetStack::new();

        /* 
        UDP_SOCKET
            .set(Arc::new(udp_socket))
            .map_err(|_| "failed initializing udp recv half")
            .unwrap();

        UDP_SESSIONS
            .set(Mutex::new(HashMap::new()))
            .map_err(|_| "failed initializing UDP session pool")
            .unwrap();
        */
        Ok(Self {
            net_stack,
            tcp_listener,
            udp_socket,
            dev,
            next_assoc_id: AtomicU16::new(0),
        })
    }

    pub async fn start() {
        log::warn!("[Tap] netstak init");
        let mut tap_proxy = TapProxy::new().unwrap();

        let mut framed = tap_proxy.dev.into_framed();
        let (mut tun_sink, mut tun_stream) = framed.split();
        let (mut stack_sink, mut stack_stream) = tap_proxy.net_stack.split();
        
        // Reads packet from stack and sends to TUN.
        tokio::spawn(async move {
            while let Some(pkt) = stack_stream.next().await {
                if let Ok(pkt) = pkt {
                    tun_sink.send(TunPacket::new(pkt)).await.unwrap();
                }
            }
        });

        // Reads packet from TUN and sends to stack.
        tokio::spawn(async move {
            while let Some(pkt) = tun_stream.next().await {
                if let Ok(pkt) = pkt {
                    stack_sink.send(pkt.into_bytes().into()).await.unwrap();
                }
            }
        });

        // Extracts TCP connections from stack and sends them to the dispatcher.
        tokio::spawn(async move {
            while let Some((stream, local_addr, remote_addr)) = tap_proxy.tcp_listener.next().await {
                tokio::spawn(handle_inbound_stream(
                    stream,
                    local_addr,
                    remote_addr,
                ));
            }
        });

        // Receive and send UDP packets between netstack and NAT manager. The NAT
        // manager would maintain UDP sessions and send them to the dispatcher.
        tokio::spawn(async move {
            handle_inbound_datagram(tap_proxy.udp_socket, tap_proxy.next_assoc_id).await;
        });
        loop {};
    }
}



async fn handle_inbound_stream(
    mut stream: netstack::TcpStream,
    local_addr:SocketAddr, 
    remote_addr: SocketAddr,
) { 
    log::info!("inboud stream {local_addr} {remote_addr}");
    
    if remote_addr.is_ipv4() {
        let target_addr = TuicAddress::SocketAddress(remote_addr);
        let relay = match TuicConnection::get().await {
            Ok(conn) => conn.connect(target_addr.clone()).await,
            Err(err) => Err(err),
        };
        /*todo!后续处理参考下，错误关闭的流程*/
        match relay {
            Ok(relay) => {
                let mut relay = relay.compat();
                match io::copy_bidirectional(&mut stream, &mut relay).await {
                    Ok(_) => {
                        log::warn!("recv data");
                    },
                    Err(err) => {
                        let _ = stream.shutdown().await;
                        let _ = relay.get_mut().reset(ERROR_CODE);
                        log::warn!("TCP stream relaying error 111");
                    },
                };
            },
            Err(err) => {
                log::warn!("unable to relay TCP stream");
            }
        }
    }
}

pub static UDP_SOCKET: OnceCell<netstack::SendHalf> = OnceCell::new();
lazy_static! {
    static ref GLOBAL_PORT: Mutex<u16> = Mutex::new(0);
}
async fn handle_inbound_datagram(socket: Box<netstack::UdpSocket>, next_assoc_id: AtomicU16) {
    let (ls, mut lr) = socket.split();

    UDP_SOCKET.set(ls);

    //recv datagrams from stack and send To tuic
    loop {
        match lr.recv_from().await {
            Err (e) => {
                log::warn!("Failed to accept a datagram from netstack: {}", e);
            }
            Ok((data, src_addr, dst_addr)) => {
                log::debug!("Recv udp pkt src:{src_addr}  dst{dst_addr}");
                println!("Recv udp pkt src:{src_addr} dst{dst_addr}");
                /* 
                let entry = UDP_SESSIONS
                    .get()
                    .unwrap()
                    .lock()
                    .entry(src_addr.port())
                    .or_insert_with(||
                        UdpSession::new(next_assoc_id.fetch_add(1, Ordering::Relaxed), src_addr.port()).unwrap()
                    );
                */ 
                let mut port_guard = GLOBAL_PORT.lock();
                *port_guard = src_addr.port();

                let forward = async move {

                    let target_addr = TuicAddress::SocketAddress(dst_addr);

                    match TuicConnection::get().await {
                        Ok(conn) => conn.packet(data.into(), target_addr, 1).await,
                        Err(err) => Err(err),
                    }
                };

                tokio::spawn(async move {
                    match forward.await {
                        Ok(()) => {}
                        Err(err) => {
                            log::warn!("[TAP] [associate]failed relaying UDP packet: {err}");
                        }
                    }
                });
            }
        }
    }

}

pub async fn handle_udp_inbound_datagram(pkt: Bytes, src_addr: SocketAddr) { 
    let dst_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(240, 0, 0, 1)), *GLOBAL_PORT.lock());
    println!("Recv udp pkt src:{src_addr} dst{dst_addr}");
    
    UDP_SOCKET.get().unwrap().send_to(&pkt, &src_addr, &dst_addr);
}