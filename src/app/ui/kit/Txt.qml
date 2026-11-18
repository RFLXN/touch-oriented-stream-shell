// 기본 텍스트: `Theme`에서 기본 폰트와 색상을 가져온다. UI의 모든 텍스트는 이 컴포넌트를 사용한다.
import QtQuick

Text {
    color: Theme.ink
    font.family: Theme.font
    font.pixelSize: Theme.fBody
}
