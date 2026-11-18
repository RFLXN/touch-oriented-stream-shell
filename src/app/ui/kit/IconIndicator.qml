// 아이콘 인디케이터: LED 형태로 점등되는 아이콘. 별도 라벨 없이 아이콘 모양만으로 대상 상태를 표시한다(상단바 인터페이스 상태 등).
// `Indicator`와 동일하게 동작한다: 꺼지면 흐린 색, 켜지면 `color`로 발광하며 `blink`가 true면 점멸한다.
// `name`은 `Glyph` 아이콘 이름이다.
import QtQuick
import QtQuick.Effects

Item {
    id: root
    property string name: ""
    property bool on: false
    property bool blink: false
    property color color: Theme.indicatorGreen
    property real size: 24

    readonly property bool lit: on && (!blink || blinker.phase)
    property real level: lit ? 1 : 0
    Behavior on level { NumberAnimation { duration: 110 } }

    implicitWidth: size
    implicitHeight: size

    Blinker { id: blinker; active: root.blink && root.on }

    RectangularShadow {
        anchors.centerIn: parent
        width: root.size * 0.5
        height: width
        radius: width / 2
        blur: root.size * 0.9
        color: root.color
        opacity: root.level * 0.5
        visible: root.level > 0.01
    }
    Glyph {
        name: root.name
        size: root.size
        grow: 1
        color: Theme.indicatorSeat
    }
    Glyph {
        name: root.name
        size: root.size
        color: Theme.mix(Theme.indicatorOff, root.color, root.level)
    }
}
