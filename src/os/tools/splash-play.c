/*
 * splash-play <프레임 파일> [프레임버퍼] — 부팅 스플래시 애니메이션을 프레임버퍼에 반복 재생한다.
 *
 * initramfs가 첫 프레임(`fbsplash`)을 그린 뒤 백그라운드로 띄우고, 화면을 넘겨받기 전에 `tf-kiosk`가 종료시킨다
 * (`killall splash-play`). 프레임버퍼는 기본값 `/dev/fb0`이며, 파일은 처음에 통째로 메모리에 매핑하므로
 * switch_root로 initramfs가 지워진 뒤에도 계속 재생한다.
 *
 * 프레임 파일은 `src/scripts/splash.py`가 만든다(리틀 엔디언 u32).
 *   헤더 64바이트: "TFSP", 버전 2, 프레임버퍼 한 줄 바이트 수, 프레임 수, 나머지는 0
 *   프레임마다:    x, y, 너비, 높이, 재생 시간(ms) + `너비 × 높이 × 4`바이트(프레임버퍼 픽셀 형식, 위에서 아래로)
 * 프레임 k의 사각형은 프레임 k-1(첫 프레임은 마지막 프레임)에서 달라진 부분이다. 달라진 곳이 없으면 너비와 높이가 0이다.
 * 첫 프레임은 이미 화면에 있으므로 그 재생 시간만큼 기다린 뒤 두 번째 프레임부터 그린다.
 * 형식이 맞지 않으면 아무것도 그리지 않고 종료한다.
 *
 * libc 없이 빌드한다(`reboot-mode`와 같은 방식):
 *   clang --target=aarch64-unknown-linux-gnu -static -nostdlib -ffreestanding -O2 -fuse-ld=lld
 */

static long syscall6(long n, long a, long b, long c, long d, long e, long f) {
  register long x8 __asm__("x8") = n;
  register long x0 __asm__("x0") = a;
  register long x1 __asm__("x1") = b;
  register long x2 __asm__("x2") = c;
  register long x3 __asm__("x3") = d;
  register long x4 __asm__("x4") = e;
  register long x5 __asm__("x5") = f;
  __asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4), "r"(x5) : "memory");
  return x0;
}

enum {
  SYS_openat = 56,
  SYS_lseek = 62,
  SYS_pwrite64 = 68,
  SYS_exit = 93,
  SYS_nanosleep = 101,
  SYS_mmap = 222,
  AT_FDCWD = -100,
  O_RDONLY = 0,
  O_WRONLY = 1,
  SEEK_END = 2,
  PROT_READ = 1,
  MAP_PRIVATE = 2,
  HEADER = 64,
  RECORD = 20,
  MAGIC = 0x50534654,
  MAX_FRAMES = 4096,
};

static const unsigned int *frame[MAX_FRAMES];

static void quit(int code) {
  for (;;)
    syscall6(SYS_exit, code, 0, 0, 0, 0, 0);
}

static void pause_ms(unsigned long ms) {
  long wait[2];
  wait[0] = ms / 1000;
  wait[1] = (ms % 1000) * 1000000;
  syscall6(SYS_nanosleep, (long)wait, 0, 0, 0, 0, 0);
}

void start_c(long *sp) {
  long argc = sp[0];
  char **argv = (char **)(sp + 1);
  if (argc < 2)
    quit(2);
  const char *fbdev = argc > 2 ? argv[2] : "/dev/fb0";

  long fd = syscall6(SYS_openat, AT_FDCWD, (long)argv[1], O_RDONLY, 0, 0, 0);
  if (fd < 0)
    quit(1);
  long size = syscall6(SYS_lseek, fd, 0, SEEK_END, 0, 0, 0);
  if (size < HEADER)
    quit(1);
  long map = syscall6(SYS_mmap, 0, size, PROT_READ, MAP_PRIVATE, fd, 0);
  if (map < 0 && map > -4096)
    quit(1);
  const unsigned int *h = (const unsigned int *)map;
  unsigned long stride = h[2], count = h[3];
  if (h[0] != MAGIC || h[1] != 2 || count < 2 || count > MAX_FRAMES)
    quit(1);

  unsigned long at = HEADER;
  for (unsigned long k = 0; k < count; k++) {
    if (at + RECORD > (unsigned long)size)
      quit(1);
    const unsigned int *r = (const unsigned int *)(map + at);
    unsigned long x = r[0], w = r[2], bytes = (unsigned long)r[2] * r[3] * 4;
    if ((x + w) * 4 > stride || at + RECORD + bytes > (unsigned long)size)
      quit(1);
    frame[k] = r;
    at += RECORD + bytes;
  }
  if (at != (unsigned long)size)
    quit(1);

  long fb = syscall6(SYS_openat, AT_FDCWD, (long)fbdev, O_WRONLY, 0, 0, 0);
  if (fb < 0)
    quit(1);

  for (unsigned long k = 0;;) {
    pause_ms(frame[k][4]);
    k = (k + 1) % count;
    const unsigned int *r = frame[k];
    unsigned long x = r[0], y = r[1], w = r[2], rows = r[3];
    const unsigned char *src = (const unsigned char *)(r + 5);
    for (unsigned long i = 0; i < rows; i++)
      if (syscall6(SYS_pwrite64, fb, (long)(src + i * w * 4), w * 4, (y + i) * stride + x * 4, 0, 0) < 0)
        quit(1);
  }
}

__attribute__((naked)) void _start(void) {
  __asm__("mov x0, sp\n"
          "bl start_c\n");
}
