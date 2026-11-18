// 인쇄형 라벨: 패널이나 디스플레이에 작게 표기하는 굵은 대문자 텍스트(위젯 프레임 라벨, 설정 섹션 제목, 테이블 좌측 컬럼 등).
// 기본 색상은 패널용 흐린 텍스트 색이며, 디스플레이 내부에서는 `Theme.displayDim`으로 덮어써 사용한다.
import QtQuick

Txt {
    color: Theme.inkDim
    font.pixelSize: Theme.fLabel
    font.bold: true
    font.letterSpacing: 1.5
}
