// 디스플레이: 유리로 덮인 창 영역. 텍스트, 목록, 이미지처럼 동적으로 변하는 콘텐츠는 모두 이 안에 배치한다.
//
// 유리는 앞판과 같은 평면에 놓이고 콘텐츠는 유리 아래 들어간 면에 배치되며, 깊이감은 그림자만으로 표현한다:
// 창의 상단과 왼쪽 가장자리가 콘텐츠 위에 그림자를 드리운다(별도 측벽을 그리니 테두리가 두꺼워 보여 생략했다).
// 유리 반사광은 최상단에서 창 전체를 덮는다(고정된 반투명 사각형 두 개라 렌더링 부하가 거의 없다).
// `Theme.glass`를 끄면 전체 화면의 반사광이 꺼지며(설정의 Glass effect), 특정 디스플레이만 끄려면 `glass` 프로퍼티를 끈다(텍스트 입력 칸 등).
// 내부는 평면으로 유지하며(돌출 컴포넌트 배치 금지) 콘텐츠는 영역 밖으로 넘치지 않게 클리핑한다.
import QtQuick

Item {
    id: root
    default property alias content: inner.data
    property bool glass: true

    Rectangle {
        anchors.fill: parent
        radius: Theme.rDisplay + 1
        color: Theme.displaySeam
    }
    Rectangle {
        id: floor
        anchors { fill: parent; margins: 1 }
        radius: Theme.rDisplay
        color: Theme.display
        Item { id: inner; anchors.fill: parent; clip: true }
        Rectangle {
            width: parent.width
            height: Math.min(parent.height / 2, Theme.displayShadow)
            radius: parent.radius
            gradient: Gradient {
                GradientStop { position: 0; color: Theme.displayShade }
                GradientStop { position: 1; color: "transparent" }
            }
        }
        Rectangle {
            width: Math.min(parent.width / 2, Theme.displayShadow * 0.55)
            height: parent.height
            radius: parent.radius
            opacity: 0.55
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Theme.displayShade }
                GradientStop { position: 1; color: "transparent" }
            }
        }
    }
    Item {
        id: glare
        anchors { fill: parent; margins: 1 }
        visible: Theme.glass && root.glass
        clip: true
        readonly property real angle: 32
        readonly property real slope: Math.tan(angle * Math.PI / 180)
        readonly property real thick: Math.min(height * 0.6, 84)
        readonly property real reach: Math.max(width * 0.34, thick * 1.5)

        component Streak: Rectangle {
            property real reach
            width: 2 * (glare.width + glare.height)
            x: reach / 2 - width / 2
            y: reach * glare.slope / 2 - height / 2
            rotation: -glare.angle
            antialiasing: true
            gradient: Gradient {
                GradientStop { position: 0; color: "transparent" }
                GradientStop { position: 0.5; color: Theme.displayGlare }
                GradientStop { position: 1; color: "transparent" }
            }
        }
        Streak { reach: glare.reach; height: glare.thick }
        Streak { reach: glare.reach + glare.thick * 1.25; height: glare.thick * 0.3 }
    }
}
