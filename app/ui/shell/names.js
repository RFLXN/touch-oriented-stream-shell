// 데몬 상태 코드를 UI 표시용 텍스트로 변환하는 헬퍼. 여러 화면에서 표기를 통일하기 위해 한 곳에 모았다.
//   viaText(via)   호스트 연결 경로: `"usb"` -> `"USB"`, `"lan"` -> `"Ethernet"`, `"wlan"` -> `"Wi-Fi"`
// 사용 예: `import "names.js" as Names` -> `Names.viaText(host.via)`
.pragma library

function viaText(via) {
    return via === "usb" ? "USB" : via === "lan" ? "Ethernet" : via === "wlan" ? "Wi-Fi" : via
}
