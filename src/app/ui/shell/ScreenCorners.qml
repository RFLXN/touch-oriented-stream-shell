// 화면 모서리 라운딩 마스크. 디스플레이 패널은 직사각형이지만 기기 베젤 모서리가 둥글어 네 모서리가 베젤에 약간 가려진다.
// 가운데가 비어 있는 검은색 프레임을 최상단에 덮어, 안쪽 윤곽이 기기 베젤의 둥근 곡선과 맞물리도록 처리한다.
// (안쪽 곡률 반지름 = `radius - border.width = Theme.screenRadius`. 전체 화면을 오프스크린 버퍼로 그려 마스킹하는 것보다 비용이 훨씬 적다.)
// 부모 창을 가득 채우도록 배치한다(`anchors.fill: parent`).
import QtQuick
import "../kit"

Item {
    property color color: "black"
    Rectangle {
        anchors { fill: parent; margins: -Theme.screenRadius }
        color: "transparent"
        border.color: parent.color
        border.width: Theme.screenRadius + Theme.screenInset
        radius: 2 * Theme.screenRadius + Theme.screenInset
    }
}
