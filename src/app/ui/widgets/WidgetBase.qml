// 모든 위젯 컴포넌트의 최상위 베이스. `shell/WidgetSlot`이 아래 프로퍼티를 주입하고, 위젯에서 발생한 `action` 시그널을 데몬으로 전달한다.
// 개별 위젯은 이 컴포넌트(또는 이를 감싼 `CompositeWidget`)를 루트로 사용한다.
import QtQuick

Item {
    property var widget: ({})
    property var sys: ({})
    property var host: null
    property var result: null
    readonly property var command: {
        const id = widget.settings && widget.settings.command ? widget.settings.command : ""
        if (id === "" || !host || !host.commands)
            return null
        for (const c of host.commands)
            if (c.id === id)
                return c
        return null
    }
    property var value: undefined
    property var metrics: []
    property var metricValues: ({})
    property var meterSource: ({ own: true, name: "", live: false })
    property var audio: null
    property var levels: ({})
    signal action(string name, var args)
}
