CC      := gcc
CFLAGS  := -Wall -Wextra -O2 -fPIC -march=native -I include
LDFLAGS := -shared

SRC     := src/utils.c src/setup.c src/monitor.c src/verify.c
OBJ     := $(SRC:.c=.o)
TARGET  := libbenchmon.so
STATIC  := libbenchmon.a

.PHONY: all clean install test

all: $(TARGET) $(STATIC)

$(TARGET): $(OBJ)
	$(CC) $(LDFLAGS) -o $@ $^

$(STATIC): $(OBJ)
	ar rcs $@ $^

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

test: $(STATIC)
	$(CC) -O2 -I include -o smoke_test tests/smoke_test.c $(STATIC)
	./smoke_test

clean:
	rm -f $(OBJ) $(TARGET) $(STATIC) smoke_test

install: $(TARGET) $(STATIC)
	install -d /usr/local/lib /usr/local/include
	install -m 644 $(TARGET) /usr/local/lib/
	install -m 644 $(STATIC) /usr/local/lib/
	install -m 644 include/benchmon.h /usr/local/include/
	ldconfig
