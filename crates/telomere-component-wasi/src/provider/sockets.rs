use super::WasiHost;
use crate::bindings::imports::{
    wasi_sockets_instance_network as sockets_instance_network,
    wasi_sockets_ip_name_lookup as sockets_ip_name_lookup, wasi_sockets_network as sockets_network,
    wasi_sockets_tcp as sockets_tcp, wasi_sockets_tcp_create_socket as sockets_tcp_create_socket,
    wasi_sockets_udp as sockets_udp, wasi_sockets_udp_create_socket as sockets_udp_create_socket,
};
use std::rc::Rc;
use telomere_component::ComponentLinker;

pub(super) fn add_to_linker_sync(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    sockets_instance_network::add_to_linker(linker, Rc::clone(&host));
    sockets_network::add_to_linker(linker, Rc::clone(&host));
    sockets_udp::add_to_linker(linker, Rc::clone(&host));
    sockets_udp_create_socket::add_to_linker(linker, Rc::clone(&host));
    sockets_tcp::add_to_linker(linker, Rc::clone(&host));
    sockets_tcp_create_socket::add_to_linker(linker, Rc::clone(&host));
    sockets_ip_name_lookup::add_to_linker(linker, host);
}

pub(super) fn add_to_linker_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    sockets_instance_network::add_to_linker_async(linker, Rc::clone(&host));
    sockets_network::add_to_linker_async(linker, Rc::clone(&host));
    sockets_udp::add_to_linker_async(linker, Rc::clone(&host));
    sockets_udp_create_socket::add_to_linker_async(linker, Rc::clone(&host));
    sockets_tcp::add_to_linker_async(linker, Rc::clone(&host));
    sockets_tcp_create_socket::add_to_linker_async(linker, Rc::clone(&host));
    sockets_ip_name_lookup::add_to_linker_async(linker, host);
}

impl sockets_instance_network::Host for WasiHost {}
impl sockets_instance_network::HostAsync for WasiHost {}
impl sockets_network::Host for WasiHost {}
impl sockets_network::HostAsync for WasiHost {}
impl sockets_udp::Host for WasiHost {}
impl sockets_udp::HostAsync for WasiHost {}
impl sockets_udp_create_socket::Host for WasiHost {}
impl sockets_udp_create_socket::HostAsync for WasiHost {}
impl sockets_tcp::Host for WasiHost {}
impl sockets_tcp::HostAsync for WasiHost {}
impl sockets_tcp_create_socket::Host for WasiHost {}
impl sockets_tcp_create_socket::HostAsync for WasiHost {}
impl sockets_ip_name_lookup::Host for WasiHost {}
impl sockets_ip_name_lookup::HostAsync for WasiHost {}
