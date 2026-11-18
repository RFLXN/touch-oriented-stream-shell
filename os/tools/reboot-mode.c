/*
 * reboot-mode <모드> — 재시작 인자를 넘겨 재부팅한다. 기본값은 "loader".
 *
 * Rockchip 부트로더는 재시작 인자에 따라 다음 부팅 방식을 고른다.
 *   loader    Loader 모드(rkdeveloptool로 기록 가능). 초기화 버튼을 누를 필요가 없다.
 *   recovery  recovery 파티션 부팅
 * busybox의 `reboot`는 재부팅 모드 인자를 전달할 수 없어 직접 작성했다.
 *
 * libc 없이 빌드한다(크로스 컴파일용 libc를 별도로 준비하지 않기 위해):
 *   clang --target=aarch64-unknown-linux-gnu -static -nostdlib -ffreestanding -O2 -fuse-ld=lld
 */

static long syscall4(long n, long a, long b, long c, long d) {
  register long x8 __asm__("x8") = n;
  register long x0 __asm__("x0") = a;
  register long x1 __asm__("x1") = b;
  register long x2 __asm__("x2") = c;
  register long x3 __asm__("x3") = d;
  __asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3) : "memory");
  return x0;
}

enum { SYS_sync = 81, SYS_exit = 93, SYS_reboot = 142 };

void start_c(long *sp) {
  long argc = sp[0];
  char **argv = (char **)(sp + 1);
  const char *mode = argc > 1 ? argv[1] : "loader";

  syscall4(SYS_sync, 0, 0, 0, 0);
  syscall4(SYS_reboot, 0xfee1dead, 672274793, 0xA1B2C3D4, (long)mode);
  syscall4(SYS_exit, 1, 0, 0, 0);
}

__attribute__((naked)) void _start(void) {
  __asm__("mov x0, sp\n"
          "bl start_c\n");
}
