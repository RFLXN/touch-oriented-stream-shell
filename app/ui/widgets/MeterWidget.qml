// 자원 미터 위젯(`monitor` 앱의 `monitor.2x1`, `2x2`, `4x1`, `4x2`, 복합 위젯):
// 선택한 대상(덱 자신 또는 페어링된 PC)의 시스템 자원을 디스플레이에 한 줄씩(짧은 이름, 세그먼트 바, 수치) 표시한다.
//
// 표시할 대상과 항목은 위젯 설정에 저장된다: `widget.settings.host`(대상 호스트, `"@deck"`이면 덱 자신), `widget.settings.metrics`(메트릭 ID 목록).
// 한 줄의 높이는 위젯 크기에 관계없이 동일하다(타일 1칸 높이에 3줄). 따라서 `2x1`에는 3개, `2x2`에는 6개가 1열로 들어가며,
// 가로 4칸 위젯(`4x1`, `4x2`)은 2열로 나뉜다(앞 절반이 왼쪽 열, 나머지 절반이 오른쪽 열 — `4x1`은 3 / 3).
// 선택한 항목 수가 최대치보다 적으면 남은 줄은 비워 둔다.
//
// 수치와 메트릭 메타데이터(단위, 상한값)는 데몬이 내려 준다(`metrics`, `metricValues` — `widgets/WidgetBase`).
// 실제 샘플링은 데몬과 PC가 수행하며, 이 위젯이 현재 표시 중인 항목만 측정한다.
//   - 눈금 바: `값 / 최댓값`. 온도는 30°C를 하한 기준으로 삼는다. 상한이 고정되지 않은 항목(네트워크 속도 등)은 관측된 최대치를 기준으로 스케일링한다.
//   - 수치 표기: 바이트(`"B"`)와 초당 바이트(`"B/s"`)는 크기에 맞게 K/M/G 단위로 변환하고, 나머지는 해당 메트릭의 소수점 자릿수와 단위를 사용한다.
//   - 대상이 PC인데 미연결 상태이거나 리소스를 제공하지 않으면(`resource.info` 권한을 허용하지 않음) 줄이 흐려지고 `"--"`가 표시된다.
// 라벨에는 대상 이름(덱 이름 또는 PC 이름)이 표시되며, PC를 모니터링할 때는 라벨 오른쪽 인디케이터로 실시간 수신 상태를 나타낸다.
import QtQuick
import "../kit"

CompositeWidget {
    id: root

    readonly property var wanted: widget.settings && widget.settings.metrics ? widget.settings.metrics : []
    readonly property int perColumn: Math.max(1, (widget.h || 1) * 3)
    readonly property int columns: (widget.w || 2) >= 4 ? 2 : 1
    readonly property bool live: !!meterSource.live

    label: (meterSource.name || "MONITOR").toUpperCase()
    corner: Indicator {
        visible: !root.meterSource.own
        on: root.live
        color: Theme.indicatorGreen
    }

    readonly property var byId: {
        const map = {}
        for (const m of metrics)
            map[m.id] = m
        return map
    }
    property var peaks: ({})
    onMetricValuesChanged: {
        let next = null
        for (const id of wanted) {
            const m = byId[id], v = metricValues[id]
            if (m && !(m.max > 0) && v > (peaks[id] || 0)) {
                next = next || Object.assign({}, peaks)
                next[id] = v
            }
        }
        if (next)
            peaks = next
    }

    function bytes(v) {
        const units = ["B", "KB", "MB", "GB", "TB"]
        let i = 0
        while (v >= 1024 && i < units.length - 1) {
            v /= 1024
            i++
        }
        return (i === 0 ? v.toFixed(0) : v >= 100 ? v.toFixed(0) : v.toFixed(1)) + " " + units[i]
    }
    function readout(m, v) {
        if (m.unit === "B")
            return bytes(v)
        if (m.unit === "B/s")
            return bytes(v) + "/s"
        return v.toFixed(m.decimals || 0) + (m.unit === "" ? "" : m.unit === "%" || m.unit === "°C" ? m.unit : " " + m.unit)
    }
    function fraction(m, v) {
        if (m.unit === "°C")
            return (v - 30) / Math.max(1, (m.max || 100) - 30)
        const top = m.max > 0 ? m.max : Math.max(peaks[m.id] || 0, 1)
        return v / top
    }
    function fallbackName(id) {
        return id.split(".").pop().toUpperCase().slice(0, 5)
    }

    component MeterRow: Item {
        id: row
        property string metric: ""
        readonly property var info: root.byId[metric]
        readonly property var now: root.metricValues[metric]
        readonly property bool known: !!info && now !== undefined && root.live
        opacity: known ? 1 : 0.5
        Label {
            anchors { left: parent.left; verticalCenter: parent.verticalCenter }
            text: row.info ? row.info.name : root.fallbackName(row.metric)
            color: Theme.displayDim
        }
        Segments {
            anchors { left: parent.left; leftMargin: 62; verticalCenter: parent.verticalCenter }
            value: row.known ? root.fraction(row.info, row.now) : 0
        }
        Txt {
            anchors { right: parent.right; verticalCenter: parent.verticalCenter }
            text: row.known ? root.readout(row.info, row.now) : "--"
            color: Theme.displayInk
            font.family: Theme.mono
            font.pixelSize: Theme.fSmall + 1
        }
    }

    Display {
        anchors.fill: parent
        Row {
            id: grid
            anchors { fill: parent; leftMargin: 14; rightMargin: 14; topMargin: 8; bottomMargin: 8 }
            spacing: 28
            Repeater {
                model: root.columns
                Column {
                    id: column
                    required property int index
                    width: (grid.width - (root.columns - 1) * grid.spacing) / root.columns
                    height: grid.height
                    Repeater {
                        model: root.wanted.slice(column.index * root.perColumn, (column.index + 1) * root.perColumn)
                        MeterRow {
                            required property string modelData
                            width: column.width
                            height: column.height / root.perColumn
                            metric: modelData
                        }
                    }
                }
            }
        }
        Rectangle {
            visible: root.columns > 1
            anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; bottom: parent.bottom; topMargin: 10; bottomMargin: 10 }
            width: 1
            color: Theme.displayOff
        }
    }
}
