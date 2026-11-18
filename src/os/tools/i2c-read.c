/*
 * i2c-read <버스 번호> <주소(16진수)> <바이트 수> — I2C 장치에서 그대로 읽어 16진수로 출력한다.
 *
 * 드라이버가 이미 바인딩된 장치도 읽을 수 있도록 `I2C_SLAVE_FORCE`를 사용한다(진단용).
 * Alpine 기본 busybox에 i2c 도구가 없고 기기에 네트워크가 없는 환경에서 쓰기 위해 직접 작성했다.
 * `reboot-mode.c`와 동일하게 libc 없이 빌드한다.
 */

static long sc(long n, long a, long b, long c, long d) {
  register long x8 __asm__("x8") = n;
  register long x0 __asm__("x0") = a;
  register long x1 __asm__("x1") = b;
  register long x2 __asm__("x2") = c;
  register long x3 __asm__("x3") = d;
  __asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3) : "memory");
  return x0;
}

enum { SYS_ioctl = 29, SYS_openat = 56, SYS_read = 63, SYS_write = 64, SYS_exit = 93 };
enum { AT_FDCWD = -100, O_RDWR = 2, I2C_SLAVE_FORCE = 0x0706 };

static long num(const char *s, int base) {
  long v = 0;
  for (; *s; s++) {
    int d = *s >= '0' && *s <= '9' ? *s - '0' : *s >= 'a' && *s <= 'f' ? *s - 'a' + 10 : -1;
    if (d < 0 || d >= base)
      break;
    v = v * base + d;
  }
  return v;
}

static void put(const char *s, long n) { sc(SYS_write, 1, (long)s, n, 0); }

void start_c(long *sp) {
  char **argv = (char **)(sp + 1);
  static char path[] = "/dev/i2c-00";
  static unsigned char buf[256];
  static const char hex[] = "0123456789abcdef";
  char out[3];
  long fd, n, want, i;

  if (sp[0] < 4) {
    put("usage: i2c-read <bus> <addr-hex> <count>\n", 41);
    sc(SYS_exit, 2, 0, 0, 0);
  }
  n = num(argv[1], 10);
  if (n < 10) {
    path[9] = '0' + n;
    path[10] = 0;
  } else {
    path[9] = '0' + n / 10;
    path[10] = '0' + n % 10;
  }
  want = num(argv[3], 10);
  if (want > 256)
    want = 256;

  fd = sc(SYS_openat, AT_FDCWD, (long)path, O_RDWR, 0);
  if (fd < 0 || sc(SYS_ioctl, fd, I2C_SLAVE_FORCE, num(argv[2], 16), 0) < 0) {
    put("open/ioctl failed\n", 18);
    sc(SYS_exit, 1, 0, 0, 0);
  }
  n = sc(SYS_read, fd, (long)buf, want, 0);
  if (n < 0) {
    put("read failed\n", 12);
    sc(SYS_exit, 1, 0, 0, 0);
  }
  for (i = 0; i < n; i++) {
    out[0] = hex[buf[i] >> 4];
    out[1] = hex[buf[i] & 15];
    out[2] = (i % 16 == 15 || i == n - 1) ? '\n' : ' ';
    put(out, 3);
  }
  sc(SYS_exit, 0, 0, 0, 0);
}

__attribute__((naked)) void _start(void) {
  __asm__("mov x0, sp\n"
          "bl start_c\n");
}
