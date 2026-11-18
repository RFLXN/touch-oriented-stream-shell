//! 시스템 상태: 호스트명, IP 주소, 인터페이스별 링크 및 주소. `/proc`와 `/sys`에서 읽는다.
//! (온도, CPU 부하, 메모리 같은 시스템 자원은 `toss-meter`가 측정하고, 자원 미터 위젯이 표시한다)

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};

use serde::Serialize;

#[derive(Serialize, Clone, PartialEq, Default)]
pub struct SysState {
    pub host: String,
    /// 외부 통신에 사용되는 IP 주소. 네트워크가 없으면 빈 문자열.
    pub ip: String,
    /// 인터페이스별 링크 업 여부(케이블 연결 또는 AP 접속 상태). 상단바 아이콘이 이 값을 사용한다.
    pub links: Links,
    /// 인터페이스별 상세 정보(상단바 아이콘을 눌렀을 때 표시)
    pub net: Net,
}

#[derive(Serialize, Clone, PartialEq, Default)]
pub struct Net {
    pub usb: Iface,
    pub eth: Iface,
    pub wlan: Iface,
    /// USB 가젯 상태: `configured`(PC 인식됨) / `not attached`(연결 안 됨) 등 커널이 보고하는 값 그대로다.
    pub usb_state: String,
    /// 유선 링크 속도(Mb/s). 링크가 없으면 0
    pub eth_speed: u32,
    /// 블루투스 어댑터 유무(데몬은 아직 블루투스를 제어하지 않는다)
    pub bt_present: bool,
}

#[derive(Serialize, Clone, PartialEq, Default)]
pub struct Iface {
    /// 인터페이스 이름(`usb0`, `eth0`, `wlan0`)
    pub name: String,
    pub up: bool,
    /// MAC 주소. 인터페이스가 없으면 빈 문자열
    pub mac: String,
    /// `IPv4/프리픽스 길이`(예: `192.168.0.5/24`). 없으면 빈 문자열
    pub addr: String,
}

#[derive(Serialize, Clone, PartialEq, Default)]
pub struct Links {
    pub usb: bool,
    pub eth: bool,
    pub wlan: bool,
}

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// 덱의 네트워크 인터페이스: USB(PC와 연결하는 가젯), 이더넷, Wi-Fi
const USB: &str = "usb0";
const ETH: &str = "eth0";
const WLAN: &str = "wlan0";

/// 외부 통신에 쓰이는 로컬 주소를 조회한다. 실제로 패킷을 보내지는 않는다.
fn outbound_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr()))
        .map(|a| a.ip().to_string())
        .unwrap_or_default()
}

pub fn collect_sys() -> SysState {
    let net = collect_net();
    SysState { host: hostname(), ip: outbound_ip(), links: Links { usb: net.usb.up, eth: net.eth.up, wlan: net.wlan.up }, net }
}

fn collect_net() -> Net {
    let addrs = ipv4_addrs();
    let iface = |name: &str| Iface {
        name: name.to_string(),
        up: link_up(name),
        mac: read_trim(&format!("/sys/class/net/{name}/address")).unwrap_or_default(),
        addr: addrs.get(name).cloned().unwrap_or_default(),
    };
    Net {
        usb: iface(USB),
        eth: iface(ETH),
        wlan: iface(WLAN),
        usb_state: fs::read_dir("/sys/class/udc")
            .ok()
            .and_then(|mut d| d.next()?.ok())
            .and_then(|e| read_trim(&e.path().join("state").to_string_lossy()))
            .unwrap_or_default(),
        eth_speed: read_trim(&format!("/sys/class/net/{ETH}/speed")).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
        bt_present: fs::metadata("/sys/class/bluetooth/hci0").is_ok(),
    }
}

/// 인터페이스 이름을 연결 경로 이름으로 변환한다(`usb` / `lan` / `wlan`). 알 수 없으면 원래 이름을 사용한다.
fn kind_of_iface(name: &str) -> String {
    match name {
        USB => "usb",
        ETH => "lan",
        WLAN => "wlan",
        other => other,
    }
    .to_string()
}

/// 덱의 로컬 주소에 해당하는 연결 경로(`usb` / `lan` / `wlan`). 알 수 없으면 빈 문자열.
pub fn link_kind(local: IpAddr) -> String {
    let local = local.to_string();
    ipv4_addrs()
        .iter()
        .find(|(_, addr)| addr.split('/').next() == Some(local.as_str()))
        .map(|(name, _)| kind_of_iface(name))
        .unwrap_or_default()
}

/// 덱에 접속 가능한 주소 목록(USB, 이더넷, Wi-Fi 순). 호스트에 알려 주면 다음 연결 시 이 주소로 접속한다.
pub fn addresses() -> Vec<String> {
    let addrs = ipv4_addrs();
    [USB, ETH, WLAN]
        .iter()
        .filter_map(|name| addrs.get(*name))
        .filter_map(|a| a.split('/').next())
        .map(str::to_string)
        .collect()
}

/// 절전 상태의 덱을 깨울 때 사용할 Wi-Fi MAC 주소. Wi-Fi가 없으면 빈 문자열
pub fn wake_mac() -> String {
    read_trim(&format!("/sys/class/net/{WLAN}/address")).unwrap_or_default()
}

pub fn hostname() -> String {
    read_trim("/proc/sys/kernel/hostname").unwrap_or_default()
}

/// 인터페이스 이름 -> `IPv4/프리픽스 길이`. 인터페이스마다 첫 번째 주소만 사용한다.
fn ipv4_addrs() -> HashMap<String, String> {
    let mut out = HashMap::new();
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return out;
        }
        let mut p = list;
        while !p.is_null() {
            let ifa = &*p;
            if !ifa.ifa_addr.is_null() && i32::from((*ifa.ifa_addr).sa_family) == libc::AF_INET {
                let ip = Ipv4Addr::from(u32::from_be((*ifa.ifa_addr.cast::<libc::sockaddr_in>()).sin_addr.s_addr));
                let prefix = if ifa.ifa_netmask.is_null() {
                    32
                } else {
                    (*ifa.ifa_netmask.cast::<libc::sockaddr_in>()).sin_addr.s_addr.count_ones()
                };
                let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                out.entry(name).or_insert_with(|| format!("{ip}/{prefix}"));
            }
            p = ifa.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    out
}

/// 인터페이스의 링크가 활성화되어 있는지 확인한다. 인터페이스가 없거나 링크가 내려가 있으면 `false`.
fn link_up(ifname: &str) -> bool {
    read_trim(&format!("/sys/class/net/{ifname}/carrier")).as_deref() == Some("1")
}
