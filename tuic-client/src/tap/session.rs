use std::{
    convert::TryFrom,
    fmt, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    string::ToString,
};

use byteorder::{BigEndian, ByteOrder};
use bytes::BufMut;
use tokio::io::{AsyncRead, AsyncReadExt};


#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum Network {
    Tcp,
    Udp,
}


pub struct Session {
    /// The network type, representing either TCP or UDP.
    pub network: Network,
    /// The socket address of the remote peer of an inbound connection.
    pub source: SocketAddr,
    /// The socket address of the local socket of an inbound connection.
    pub local_addr: SocketAddr,
    /// The proxy target address of a proxy connection.
    pub destination: SocksAddr,
    /// The tag of the inbound handler this session initiated.
    pub inbound_tag: String,
    /// The tag of the first outbound handler this session goes.
    pub outbound_tag: String,
    /// Optional stream ID for multiplexing transports.
    pub stream_id: Option<StreamId>,
}
