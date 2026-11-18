// 위젯 슬롯: 데몬이 내려 준 위젯 메타데이터(`widget`)의 `kind`에 맞는 위젯 컴포넌트를 동적으로 로드한다.
// 랙 내부 셀과 페이지에 직접 배치되는 복합 위젯 모두 이 슬롯을 사용한다.
//
// 위젯 컴포넌트에 필요한 데이터는 `link`(`shell/DaemonLink`)에서 꺼내 바인딩한다(수신 측 정의는 `widgets/WidgetBase`):
//   widget        위젯 설정 객체 그대로
//   sys           시스템 상태
//   host          대상 호스트 정보(아래 규칙 참고)
//   value         대상 명령의 현재 값(호스트가 보고한 값, 모르면 `undefined`)
//   result        이 위젯이 보낸 최근 액션 결과(없으면 `null`)
//   metrics, metricValues, meterSource   자원 미터 대상 호스트의 메트릭 목록, 현재 값, 소스 이름 및 실시간 수신 여부
//   icons         사이트 아이콘(주소 -> `data:` URI). 웹페이지 아이콘 버튼(`web.icon`)에만 넣는다
// 위젯이 요청한 액션(`action`)은 대상 호스트 ID 및 위젯 ID와 함께 데몬으로 전송된다(`link.action`).
//
// 호스트 선택 규칙: `widget.settings.host`에 호스트 ID가 지정되어 있으면 해당 호스트만 사용한다
// (미연결 상태면 `null`). 비어 있으면 연결된 첫 번째 호스트(`link.host`)를 사용하며, 액션도 그 호스트로 전달된다.
// 자원 미터 소스 규칙: `settings.host`가 비어 있거나 `"@deck"`이면 덱 자신, 그 외에는 해당 ID의 호스트를 가리킨다.
//
// !! 새 위젯 종류를 추가할 때는 아래 `sources` 맵에도 1줄을 추가한다(`docs/03-userland-guide.md` 3.17절).
import QtQuick

Loader {
    id: slot
    property var widget: ({})
    property var link: null

    readonly property var sys: link ? link.sys : ({})
    readonly property var hosts: link ? link.hosts : []
    readonly property var host: link ? link.host : null
    readonly property var results: link ? link.results : ({})
    readonly property var values: link ? link.values : ({})
    readonly property var meters: link ? link.meters : ({})
    readonly property var meterValues: link ? link.meterValues : ({})
    readonly property var icons: link ? link.icons : ({})

    readonly property var sources: ({
        "command.run": "../widgets/CommandWidget.qml",
        "command.switch": "../widgets/CommandSwitchWidget.qml",
        "command.stepped": "../widgets/CommandSteppedWidget.qml",
        "command.linear": "../widgets/CommandLinearWidget.qml",
        "media.player": "../widgets/MediaWidget.qml",
        "monitor.2x1": "../widgets/MeterWidget.qml",
        "monitor.2x2": "../widgets/MeterWidget.qml",
        "monitor.4x1": "../widgets/MeterWidget.qml",
        "monitor.4x2": "../widgets/MeterWidget.qml",
        "web.button": "../widgets/WebButtonWidget.qml",
        "web.icon": "../widgets/WebIconWidget.qml"
    })
    readonly property var noValues: ({})

    readonly property string wantedHost: widget.settings && widget.settings.host ? widget.settings.host : ""
    readonly property var shownHost: {
        if (wantedHost === "")
            return host
        for (const h of hosts)
            if (h.id === wantedHost)
                return h.connected ? h : null
        return null
    }

    readonly property var shownValue: {
        const of = shownHost ? values[shownHost.id] : undefined
        return of && widget.settings ? of[widget.settings.command] : undefined
    }

    readonly property string meterKey: wantedHost === "" ? "@deck" : wantedHost
    readonly property var meterSource: {
        if (meterKey === "@deck")
            return { own: true, name: sys.host || "DECK", live: true }
        for (const h of hosts)
            if (h.id === meterKey)
                return { own: false, name: h.name, live: h.connected && !!meters[meterKey] }
        return { own: false, name: "", live: false }
    }

    source: sources[widget.kind] ? Qt.resolvedUrl(sources[widget.kind]) : ""
    onSourceChanged: {
        if (widget.kind && !sources[widget.kind])
            console.warn("알 수 없는 위젯 종류:", widget.kind, "(위젯", widget.id + ")")
    }

    Binding { target: slot.item; property: "widget"; value: slot.widget; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "sys"; value: slot.sys; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "host"; value: slot.shownHost; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "value"; value: slot.shownValue; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "metrics"; value: slot.meters[slot.meterKey] || []; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "metricValues"; value: slot.meterValues[slot.meterKey] || slot.noValues; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "meterSource"; value: slot.meterSource; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "result"; value: slot.results[slot.widget.id] || null; when: slot.status === Loader.Ready }
    Binding { target: slot.item; property: "icons"; value: slot.icons; when: slot.status === Loader.Ready && slot.widget.kind === "web.icon" }
    Connections {
        target: slot.item
        function onAction(name, args) {
            if (slot.link)
                slot.link.action(name, args, slot.wantedHost, slot.widget.id)
        }
    }
}
