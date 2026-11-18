// 장식용 나사 머리: 위젯 프레임을 앞판(랙)에 고정한 하드웨어 느낌을 주는 요소. 다른 컴포넌트처럼 얕은 홈 안에 배치된다.
// 클릭되지 않으며 상호작용이 없다.
import QtQuick

Item {
    id: root
    property real angle: 30
    readonly property real size: Theme.screwSize

    width: size
    height: size

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.edgeLo }
            GradientStop { position: 1; color: Theme.edgeHi }
        }
    }
    Rectangle {
        anchors { fill: parent; margins: 1 }
        radius: width / 2
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.screwHi }
            GradientStop { position: 1; color: Theme.screwLo }
        }
    }
    Rectangle {
        anchors.centerIn: parent
        width: root.size - 3
        height: 1.5
        radius: 0.75
        color: Theme.screwSlot
        rotation: root.angle
        antialiasing: true
    }
}
