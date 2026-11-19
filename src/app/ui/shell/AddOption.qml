// 위젯 추가 패널(`shell/AddPanel`)의 개별 설정 섹션: 데몬 `catalog`가 정의한 위젯 설정(`modelData`) 1건을 선택하는 UI.
// 설정 타입(`modelData.type`)에 따라 입력 형태가 달라진다:
//   host     페어링된 PC 중 하나 선택(필수가 아니면 `"Any connected PC"` 포함).
//            `own`이 true면 덱 자신(`"This deck"`, 값 `"@deck"`)도, `spotify`가 true면 덱이 직접 조회하는 Spotify 계정(`"@spotify"`)도 선택 가능.
//   metrics  위에서 선택한 호스트(덱 또는 PC)의 측정 가능 메트릭 중 복수 선택(`max`개까지). 탭한 순서대로 추가되며 다시 탭하면 해제된다.
//   command  위에서 선택한 PC에 등록된 명령 중 하나 선택(해당 호스트 목록 기준). `filter`가 있으면 해당 타입의 명령만 표시.
//   text     텍스트 입력 필드
//   url      웹페이지 주소 입력 필드(자동 완성 없이 입력. `https://`로 시작한 채 열린다)
//   color    버튼 색상 선택
//   light    인디케이터 색상 선택(`AUTO`는 빈 값: 버튼 색상에 맞춘다)
//   flag     켜기/끄기 선택(`Off` / `On` 행)
// `when_host`가 있는 설정은 위에서 고른 대상(`host`)이 그 값일 때만 보인다(예: Spotify를 고른 미디어 위젯의 `Spotify theme`).
// 현재 선택 상태는 상위 `panel.chosen`에 보관되며, 여기서는 표시 및 선택 이벤트(`panel.chooseFor`, `panel.toggleMetric`, `panel.choose`)를 전달한다.
// `leave()`는 텍스트 입력 필드의 포커스와 조합 중인 문자를 정리한다(패널 닫힘 또는 ADD 클릭 시 호출).
import QtQuick
import "../kit"

SettingsSection {
    id: option
    required property var modelData
    property var panel: null
    visible: !modelData.when_host || chosen.host === modelData.when_host

    readonly property var chosen: panel ? panel.chosen : ({})
    readonly property var picked: modelData.type === "metrics" ? chosen[modelData.key] || [] : []
    readonly property var offered: modelData.type === "metrics" && panel ? panel.meters[chosen.host || ""] || [] : []
    readonly property var choices: {
        if (!panel)
            return []
        if (modelData.type === "host") {
            const any = (modelData.required ? [] : [{ value: "", title: "Any connected PC", note: modelData.about }])
                .concat(modelData.own ? [{ value: "@deck", title: "This deck", note: panel.deckName }] : [])
                .concat(modelData.spotify ? [{ value: "@spotify", title: "Spotify", note: "The Spotify account linked to this deck" }] : [])
            return any.concat(panel.hosts.map((h) => ({ value: h.id, title: h.name, note: h.connected ? "Connected" : "Not connected" })))
        }
        if (modelData.type === "command")
            return commandsOf(chosen.host || "", modelData.filter).map((c) => ({ value: c.id, title: c.name, note: shapeOf(c) }))
        if (modelData.type === "flag")
            return [{ value: false, title: "Off", note: "" }, { value: true, title: "On", note: modelData.about }]
        return []
    }

    function commandsOf(hostId, filter) {
        for (const h of panel.hosts)
            if (h.id === hostId)
                return (h.commands || []).filter((c) => !filter || (c.kind || "run") === filter)
        return []
    }
    function shapeOf(c) {
        if (c.kind === "stepped")
            return (c.steps || []).join(" / ")
        if (c.kind === "linear")
            return (c.min || 0) + " – " + (c.max || 0) + (c.step && c.step !== 1 ? "  (step " + c.step + ")" : "")
        return ""
    }
    function leave() { field.blur() }

    label: modelData.title.toUpperCase() + (modelData.type === "metrics" ? "  ·  " + picked.length + " / " + modelData.max : "")

    Repeater {
        model: option.choices
        SettingsRow {
            required property var modelData
            required property int index
            title: modelData.title
            note: modelData.note
            divider: index < option.choices.length - 1
            onTapped: option.panel.chooseFor(option.modelData, modelData.value, modelData.title)
            Indicator {
                anchors.verticalCenter: parent.verticalCenter
                on: option.chosen[option.modelData.key] === modelData.value
            }
        }
    }
    Repeater {
        model: option.offered
        SettingsRow {
            id: metricRow
            required property var modelData
            required property int index
            readonly property int order: option.picked.indexOf(modelData.id)
            title: modelData.title
            note: modelData.name + (modelData.unit ? "  ·  " + modelData.unit : "")
            divider: index < option.offered.length - 1
            opacity: order >= 0 || option.picked.length < option.modelData.max ? 1 : Theme.dimmed
            onTapped: option.panel.toggleMetric(option.modelData.key, modelData.id, option.modelData.max)
            Txt {
                visible: metricRow.order >= 0
                anchors.verticalCenter: parent.verticalCenter
                text: metricRow.order + 1
                color: Theme.inkDim
                font.family: Theme.mono
                font.pixelSize: Theme.fSmall
                font.bold: true
            }
            Indicator {
                anchors.verticalCenter: parent.verticalCenter
                on: metricRow.order >= 0
            }
        }
    }
    SettingsRow {
        visible: option.modelData.type === "metrics" && option.offered.length === 0
        divider: false
        title: !option.chosen.host ? "Choose a source first" : "Nothing to measure there right now"
        note: !option.chosen.host || option.chosen.host === "@deck" ? "" : "The PC must be connected and allow resource.info (toss-connector allow resource.info)"
    }
    SettingsRow {
        visible: (option.modelData.type === "host" || option.modelData.type === "command") && option.choices.length === 0
        divider: false
        title: option.modelData.type === "host" ? "No paired PC" : !option.chosen.host ? "Choose a PC first" : "No commands offered"
        note: option.modelData.type === "host" ? "Pair one in Settings → Paired devices"
            : !option.chosen.host ? ""
            : "On that PC: " + option.modelData.about + " …, then toss-connector allow command.run"
    }
    Item {
        visible: option.modelData.type === "text" || option.modelData.type === "url"
        width: parent.width
        height: 84
        TextField {
            id: field
            anchors { left: parent.left; right: parent.right; margins: 16; verticalCenter: parent.verticalCenter }
            placeholder: option.modelData.about
            maximumLength: option.modelData.type === "url" ? 512 : Theme.labelMax
            plain: option.modelData.type === "url"
            text: option.chosen[option.modelData.key] || ""
            onTextChanged: {
                if (visible && option.panel && text !== (option.chosen[option.modelData.key] || ""))
                    option.panel.choose(option.modelData.key, text)
            }
        }
    }
    Item {
        visible: option.modelData.type === "color"
        width: parent.width
        height: 84
        Row {
            anchors { left: parent.left; leftMargin: 16; verticalCenter: parent.verticalCenter }
            spacing: 10
            Repeater {
                model: Object.keys(Theme.buttonColors)
                Button {
                    required property string modelData
                    width: 84
                    height: 52
                    color: Theme.buttonColor(modelData)
                    held: option.chosen[option.modelData.key] === modelData
                    onTapped: option.panel.choose(option.modelData.key, modelData)
                }
            }
        }
    }
    Item {
        visible: option.modelData.type === "light"
        width: parent.width
        height: 84
        Row {
            anchors { left: parent.left; leftMargin: 16; verticalCenter: parent.verticalCenter }
            spacing: 10
            Button {
                width: 84
                height: 52
                text: "AUTO"
                held: !option.chosen[option.modelData.key]
                onTapped: option.panel.choose(option.modelData.key, "")
            }
            Repeater {
                model: Object.keys(Theme.indicatorColors)
                Button {
                    id: swatch
                    required property string modelData
                    width: 84
                    height: 52
                    held: option.chosen[option.modelData.key] === modelData
                    onTapped: option.panel.choose(option.modelData.key, modelData)
                    Indicator {
                        anchors.centerIn: parent
                        size: 14
                        on: true
                        color: Theme.indicatorColors[swatch.modelData]
                    }
                }
            }
        }
    }
}
