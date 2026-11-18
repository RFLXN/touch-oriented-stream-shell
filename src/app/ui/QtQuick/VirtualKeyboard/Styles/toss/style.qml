// 가상 키보드(Qt Virtual Keyboard) 스타일. 각 키를 다른 화면 버튼과 동일한 컴포넌트(`kit/Button`)로 그린다.
//
// 이 파일의 위치는 Qt 규약(`QML_IMPORT_PATH` 아래 `QtQuick/VirtualKeyboard/Styles/<이름>/style.qml`)을 따른다.
// 스타일 이름(`toss`)은 `QT_VIRTUALKEYBOARD_STYLE` 환경 변수로 지정한다(둘 다 `src/app/rootfs/usr/bin/toss-shell`에서 설정).
// 키 배열과 언어별 입력 처리는 Qt 쪽에 맡기고 여기서는 외관만 정의한다.
//
// 문자 키는 밝은 버튼, 기능 키(백스페이스, 시프트, 기호, 언어, 키보드 닫기)는 짙은 버튼, 엔터 키는 주황 버튼을 사용한다.
import QtQuick
import QtQuick.VirtualKeyboard
import QtQuick.VirtualKeyboard.Styles
import "../../../../kit"

KeyboardStyle {
    id: tossStyle

    keyboardDesignWidth: 800
    keyboardDesignHeight: 330
    keyboardDesignMaximumHeight: 330
    keyboardBackground: Rectangle { color: Theme.plateShade }

    component Cap: Button {
        property var key
        anchors { fill: parent; margins: 3 }
        interactive: false
        down: !!key && key.pressed
        opacity: !key || key.enabled ? 1 : 0.4
    }
    component Legend: Txt {
        anchors.centerIn: parent
        font.pixelSize: 25
        font.weight: Font.Medium
    }

    keyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            Legend { text: p.control.displayText; color: cap.inkColor }
            Txt {
                anchors { right: parent.right; top: parent.top; rightMargin: 7; topMargin: 2 }
                visible: p.control.smallTextVisible
                text: p.control.smallText
                color: Theme.inkDim
                font.pixelSize: 14
            }
        }
    }
    spaceKeyPanel: KeyPanel {
        id: p
        Cap { key: p.control }
    }
    backspaceKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("graphite")
            Glyph { anchors.centerIn: parent; name: "backspace"; size: 28; color: cap.inkColor }
        }
    }
    enterKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("orange")
            Glyph { anchors.centerIn: parent; name: "enter"; size: 28; lineWidth: 2.4; color: cap.inkColor }
        }
    }
    hideKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("graphite")
            Glyph { anchors.centerIn: parent; name: "hide"; size: 28; color: cap.inkColor }
        }
    }
    languageKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("graphite")
            Glyph { anchors.centerIn: parent; name: "globe"; size: 28; lineWidth: 1.6; color: cap.inkColor }
        }
    }
    shiftKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("graphite")
            held: InputContext.shiftActive || InputContext.capsLockActive
            Glyph { anchors.centerIn: parent; name: "shift"; size: 28; color: cap.inkColor }
            Indicator {
                anchors { right: parent.right; top: parent.top; margins: 9 }
                size: 8
                on: InputContext.shiftActive || InputContext.capsLockActive
                color: InputContext.capsLockActive ? Theme.indicatorAmber : Theme.indicatorGreen
            }
        }
    }
    symbolKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("graphite")
            Legend { text: p.control.displayText; color: cap.inkColor; font.pixelSize: 19 }
        }
    }
    modeKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("graphite")
            held: p.control.mode
            Legend { text: p.control.displayText; color: cap.inkColor; font.pixelSize: 19 }
            Indicator {
                anchors { right: parent.right; top: parent.top; margins: 9 }
                size: 8
                on: p.control.mode
            }
        }
    }
    handwritingKeyPanel: KeyPanel {
        id: p
        Cap {
            id: cap
            key: p.control
            color: Theme.buttonColor("graphite")
            Legend { text: "HW"; color: cap.inkColor; font.pixelSize: 19 }
        }
    }

    characterPreviewMargin: 6
    characterPreviewDelegate: Item {
        id: preview
        property string text
        Display {
            anchors.centerIn: parent
            width: Math.max(parent.width, 64)
            height: 72
            Txt {
                anchors.centerIn: parent
                text: preview.text
                color: Theme.displayInk
                font.pixelSize: 36
                font.weight: Font.Medium
            }
        }
    }

    alternateKeysListItemWidth: 62
    alternateKeysListItemHeight: 68
    alternateKeysListTopMargin: 4
    alternateKeysListBottomMargin: 4
    alternateKeysListLeftMargin: 4
    alternateKeysListRightMargin: 4
    alternateKeysListDelegate: Item {
        id: alt
        width: tossStyle.alternateKeysListItemWidth
        height: tossStyle.alternateKeysListItemHeight
        Txt {
            anchors.centerIn: parent
            text: model.text
            color: alt.ListView.isCurrentItem ? Theme.display : Theme.displayInk
            font.pixelSize: 30
        }
    }
    alternateKeysListHighlight: Rectangle { radius: Theme.rDetail; color: Theme.displayInk }
    alternateKeysListBackground: Display { }

    selectionListHeight: 54
    selectionListDelegate: SelectionListItem {
        id: cand
        width: candText.implicitWidth + 36
        Txt {
            id: candText
            anchors.centerIn: parent
            text: display
            color: cand.ListView.isCurrentItem ? Theme.display : Theme.displayInk
            font.pixelSize: 24
        }
    }
    selectionListHighlight: Rectangle { radius: Theme.rDetail; color: Theme.displayInk }
    selectionListBackground: Display { }

    popupListDelegate: SelectionListItem {
        id: pop
        width: popText.implicitWidth + 36
        height: 52
        Txt {
            id: popText
            anchors.centerIn: parent
            text: display
            color: pop.ListView.isCurrentItem ? Theme.display : Theme.displayInk
            font.pixelSize: 24
        }
    }
    popupListHighlight: Rectangle { radius: Theme.rDetail; color: Theme.displayInk }
    popupListBackground: Display { }
    languagePopupListEnabled: true
    languageListDelegate: SelectionListItem {
        id: lang
        width: langText.implicitWidth + 44
        height: 56
        Txt {
            id: langText
            anchors.centerIn: parent
            text: displayName
            color: lang.ListView.isCurrentItem ? Theme.display : Theme.displayInk
            font.pixelSize: 22
        }
    }
    languageListHighlight: Rectangle { radius: Theme.rDetail; color: Theme.displayInk }
    languageListBackground: Display { }

    selectionHandle: Rectangle {
        implicitWidth: 14
        implicitHeight: 22
        radius: Theme.rDetail
        color: Theme.indicatorAmber
    }
    navigationHighlight: Rectangle {
        color: "transparent"
        border.width: 2
        border.color: Theme.indicatorAmber
    }
}
