#!/usr/bin/env python3
"""디컴파일된 DTS에서 노드를 검색해 숫자 phandle 참조를 라벨 이름으로 치환해 출력한다.

    dt-show.py <dts> '<노드 이름 정규식>' [...]

순정 디바이스 트리를 메인라인용으로 포팅할 때 참고용으로 사용한다. 숫자가 phandle인지 일반 정수인지는
속성 이름으로 휴리스틱 판별하므로(`...-supply`, `...gpios`, `pinctrl-N` 등) 결과는 직접 확인해야 한다.
"""
import re
import sys

REF_PROPS = re.compile(
    r"^(.*-supply|.*gpios?|pinctrl-\d+|interrupt-parent|clocks|assigned-clocks|assigned-clock-parents|"
    r"phys|extcon|io-channels|power-domains|rockchip,grf|rockchip,pmu|resets|backlight|pwms|memory-region|"
    r"phy-handle|mmc-pwrseq|vin)$"
)


def main() -> None:
    text = open(sys.argv[1]).read().split("\n")
    labels: dict[int, str] = {}
    stack: list[str] = []
    for line in text:
        s = line.strip()
        m = re.match(r"(?:([A-Za-z0-9_]+): )?([A-Za-z0-9_,@.+-]+) \{$", s)
        if m:
            stack.append(m.group(1) or m.group(2))
        elif s == "};" and stack:
            stack.pop()
        else:
            m = re.match(r"phandle = <(0x[0-9a-f]+)>;", s)
            if m and stack:
                labels[int(m.group(1), 16)] = stack[-1]

    def resolve(s: str) -> str:
        m = re.match(r"([A-Za-z0-9_,#-]+) = <(.*)>;$", s)
        if not m or not REF_PROPS.match(m.group(1)):
            return s
        cells = m.group(2).split()
        every = m.group(1).startswith("pinctrl-")
        for i, c in enumerate(cells):
            if (every or i == 0) and re.fullmatch(r"0x[0-9a-f]+", c) and int(c, 16) in labels:
                cells[i] = "&" + labels[int(c, 16)]
        return f"{m.group(1)} = <{' '.join(cells)}>;"

    for pat in sys.argv[2:]:
        depth, on = 0, False
        for line in text:
            s = line.strip()
            if not on and s.endswith("{") and re.search(pat, s):
                on, depth = True, 0
            if on:
                if not s.startswith("phandle =") and s:
                    print("  " * depth + resolve(s))
                depth += s.count("{") - s.count("};")
                if depth <= 0:
                    on = False
                    print()


if __name__ == "__main__":
    main()
