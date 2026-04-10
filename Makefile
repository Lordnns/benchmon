CC      := gcc
CFLAGS  := -Wall -Wextra -O2 -fPIC -march=native -I include
LDFLAGS := -shared
VERSION ?= 1.0.0

SRC     := src/utils.c src/setup.c src/monitor.c src/verify.c
OBJ     := $(SRC:.c=.o)
TARGET  := libbenchmon.so
STATIC  := libbenchmon.a

.PHONY: all clean install deb

all: $(TARGET) $(STATIC)

$(TARGET): $(OBJ)
	$(CC) $(LDFLAGS) -o $@ $^

$(STATIC): $(OBJ)
	ar rcs $@ $^

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(OBJ) $(TARGET) $(STATIC) *.deb
	rm -rf benchmon_deb

install: $(TARGET) $(STATIC)
	install -d /usr/local/lib /usr/local/include
	install -m 644 $(TARGET) /usr/local/lib/
	install -m 644 $(STATIC) /usr/local/lib/
	install -m 644 include/benchmon.h /usr/local/include/
	ldconfig

deb: all
	mkdir -p benchmon_deb/usr/local/lib
	mkdir -p benchmon_deb/usr/local/include
	mkdir -p benchmon_deb/DEBIAN
	cp $(TARGET) benchmon_deb/usr/local/lib/
	cp $(STATIC) benchmon_deb/usr/local/lib/
	cp include/benchmon.h benchmon_deb/usr/local/include/
	echo "Package: libbenchmon\nVersion: $(VERSION)\nArchitecture: amd64\nMaintainer: Lordnns\nDescription: Latency-sensitive benchmark monitor library\n" > benchmon_deb/DEBIAN/control
	dpkg-deb --build benchmon_deb libbenchmon.deb
	rm -rf benchmon_deb
