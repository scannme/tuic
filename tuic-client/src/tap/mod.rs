use crate::{config::Local, error::Error};
use crate::connection::{Connection as TuicConnection, ERROR_CODE};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
//use netstack_lwip::{NetStack, TcpListener, UdpSocket};
use tun::{Configuration, r#async::AsyncDevice, TunPacket};
use tokio::io::{self, AsyncWriteExt};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use futures::{SinkExt, StreamExt, Future};
use tuic::Address as TuicAddress;
use tuic_quinn::{Connect, Packet};


use std::net::SocketAddr;

pub use netstack_lwip as netstack;

pub struct TapProxy {
    net_stack: netstack::NetStack,
    tcp_listener: netstack::TcpListener,
    udp_socket: Box<netstack::UdpSocket>,
    dev: AsyncDevice,
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

        Ok(Self {
            net_stack,
            tcp_listener,
            udp_socket,
            dev,
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
        //tokio::spawn(async move {
        //    handle_inbound_datagram(udp_socket).await;
        //});
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

    /* 
    match relay {
        Ok(relay) => {
            let mut relay = relay.compat();

            match conn.reply(Reply::Succeeded, Address::unspecified()).await {
                Ok(mut conn) => match io::copy_bidirectional(&mut stream, &mut relay).await {
                    Ok(_) => {}
                    Err(err) => {
                        let _ = conn.shutdown().await;
                        let _ = relay.get_mut().reset(ERROR_CODE);
                        log::warn!("[socks5] [{peer_addr}] [connect] [{target_addr}] TCP stream relaying error: {err}");
                    }
                },
                Err(err) => {
                    let _ = relay.shutdown().await;
                    log::warn!("[socks5] [{peer_addr}] [connect] [{target_addr}] command reply error: {err}");
                }
            }
        }
        Err(err) => {
            log::warn!("[socks5] [{peer_addr}] [connect] [{target_addr}] unable to relay TCP stream: {err}");

            match conn
                .reply(Reply::GeneralFailure, Address::unspecified())
                .await
            {
                Ok(mut conn) => {
                    let _ = conn.shutdown().await;
                }
                Err(err) => {
                    log::warn!("[Tap] [{peer_addr}] [connect] [{target_addr}] command reply error: {err}")
                }
            }
        }
    }
    */
}