use crate::{config::Local, error::Error};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use netstack-lwip::{NetStack, TcpListener, UdpSocket};
use tun::{Configuration, device::AsyncDevice};

pub struct TapProxy {
    net_stack: NetStack,
    tcp_listener: TcpListener,
    udp_socket: UdpSocket,
    dev: AsyncDevice,
}

impl TapProxy {
    fn new() -> Result<Self, Error> {
        let mut config = Configuration::default();
        config
            .name("vtest")
            .address("240.0.0.1")
            .netmask("255.255.255.0")
            .up(); 
        let dev = tun::create_as_async(&config).unwrap();
        let (netstack, mut tcp_listener, udp_socket) = NetStack::new();

        Self {
            netstack,
            tcp_listener,
            udp_socket,
            dev,
        }
    }

    pub sync fn start() {
        log::warn!("[Tap] netstak init");
        let tap_proxy = TapProxy::new();

        let mut framed = tap_proxy.dev.info_framed();
        let (mut tun_sink, mut tun_stream) = framed.split();
        let (mut stack_sink, mut stack_stream) = tcp_proxy.netstack.split();
        
        // Reads packet from stack and sends to TUN.
        tokio::spawn(async move {
            while let Some(pkt) = stack_stream.next().await {
                if let Ok(pkt) = pkt {
                    tun_sink.send(pkt).await.unwrap();
                }
            }
        });

        // Reads packet from TUN and sends to stack.
        tokio::spawn(async move {
            while let Some(pkt) = tun_stream.next().await {
                if let Ok(pkt) = pkt {
                    stack_sink.send(pkt).await.unwrap();
                }
            }
        });

        // Extracts TCP connections from stack and sends them to the dispatcher.
        tokio::spawn(async move {
            while let Some((stream, local_addr, remote_addr)) = tcp_listener.next().await {
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
    }
}