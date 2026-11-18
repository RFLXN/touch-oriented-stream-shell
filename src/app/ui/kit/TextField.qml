// 텍스트 입력 필드: 디스플레이 안에 텍스트를 입력받는다. 포커스를 받으면 가상 키보드(`shell/KeyboardPanel`)가 열린다.
//
// `text` 프로퍼티로 값을 읽고 쓴다. `secret`이 true면 입력 문자를 마스킹한다(비밀번호). 키보드의 엔터 키를 누르면 `accepted` 시그널이 발생한다.
// 비밀번호처럼 일반 단어가 아닌 입력을 받을 때는 `plain`을 켜서 자동 대문자와 예측 입력을 끈다.
// `blur()`는 입력 조합 중인 문자열(preedit)을 커밋하고 키보드를 내린다.
// 확인 버튼 등을 눌러 입력 값을 읽을 때는 `text`를 읽기 전에 반드시 `blur()`를 호출해야 조합 중인 마지막 글자까지 포함된다.
import QtQuick

Display {
    id: root
    property alias text: input.text
    property string placeholder: ""
    property bool secret: false
    property bool plain: false
    property alias maximumLength: input.maximumLength
    signal accepted()

    function focusInput() { input.forceActiveFocus() }
    function blur() {
        if (input.activeFocus)
            Qt.inputMethod.commit()
        input.focus = false
    }

    implicitHeight: 56
    glass: false

    Txt {
        anchors { left: parent.left; leftMargin: 16; verticalCenter: parent.verticalCenter }
        visible: input.text === "" && input.preeditText === ""
        text: root.placeholder
        color: Theme.displayDim
        font.family: Theme.mono
    }
    TextInput {
        id: input
        anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
        verticalAlignment: TextInput.AlignVCenter
        clip: true
        color: Theme.displayInk
        selectionColor: Theme.displayDim
        selectedTextColor: Theme.display
        font.family: Theme.mono
        font.pixelSize: Theme.fBody
        echoMode: root.secret ? TextInput.Password : TextInput.Normal
        passwordCharacter: "•"
        inputMethodHints: root.plain || root.secret
            ? Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase | Qt.ImhPreferLowercase | Qt.ImhSensitiveData
            : Qt.ImhNone
        onAccepted: root.accepted()
    }
}
