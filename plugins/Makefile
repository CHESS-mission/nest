# Makefile QEMU TCG fault-injection plugin
#
# Quick start
#   make                          # auto-detect QEMU headers via pkg-config
#   make QEMU_INCLUDE=/usr/include/qemu
#   make QEMU_INCLUDE=$(nix eval --raw nixpkgs#qemu.dev)/include
#
#   fault_inject.so   QEMU TCG plugin (shared library, API v2, QEMU 8.1+)
#
# Notes
#   • The plugin is loaded via:
#       -plugin /abs/path/fault_inject.so,socket=/tmp/fi.sock
#   • Requires GLib 2.x (already a QEMU dependency always available).
#   • To build inside the Nix devShell:
#       nix develop ../nest --command make

CC      := gcc
TARGET  := fault_inject.so
SRCS    := fault_inject.c

# Locate QEMU plugin header 
# Try, in order:
#   1. User-supplied QEMU_INCLUDE
#   2. pkg-config for qemu (Nix / some distros expose this)
#   3. Common system paths
ifndef QEMU_INCLUDE
  # Prefer API v5 (write_memory_vaddr, added in QEMU 10.2).
  # Find all qemu-plugin.h files in the Nix store, pick the highest version
  # that contains write_memory_vaddr (i.e. v5+), then fall back to any other.
  _ALL_HEADERS := $(shell find /nix/store -maxdepth 4 -name "qemu-plugin.h" \
                    2>/dev/null | sort -V -r)
  _V5_HEADERS  := $(shell grep -l "write_memory_vaddr" $(_ALL_HEADERS) 2>/dev/null)
  _SYS_CANDIDATES := /usr/include/qemu /usr/local/include/qemu
  QEMU_INCLUDE := $(firstword $(patsubst %/qemu-plugin.h,%,$(_V5_HEADERS)) \
                               $(patsubst %/qemu-plugin.h,%,$(_ALL_HEADERS)) \
                               $(_SYS_CANDIDATES))
endif

ifeq ($(QEMU_INCLUDE),)
  $(error Cannot locate qemu-plugin.h. Set QEMU_INCLUDE=/path/to/qemu/include)
endif

# GLib (required by QEMU plugin API for GByteArray) 
GLIB_CFLAGS  := $(shell pkg-config --cflags glib-2.0)
GLIB_LIBDIR  := $(shell pkg-config --variable=libdir glib-2.0)
# Add explicit -L so Nix binutils (which ignores /usr/lib) can find libglib-2.0
GLIB_LIBS    := -L$(GLIB_LIBDIR) $(shell pkg-config --libs glib-2.0)

# Compiler 
CFLAGS  := -fPIC -shared \
           -Wall -Wextra -Wpedantic \
           -O2 -g \
           -I$(QEMU_INCLUDE) \
           $(GLIB_CFLAGS)

LDFLAGS := -shared -lpthread -ldl \
           $(GLIB_LIBS)

.PHONY: all clean info check-deps

all: check-deps $(TARGET)

$(TARGET): $(SRCS)
	$(CC) $(CFLAGS) -o $@ $^ $(LDFLAGS)
	@echo "Built: $(TARGET)"
	@echo "  QEMU headers: $(QEMU_INCLUDE)"
	@file $(TARGET) | grep -o 'shared object.*'

clean:
	rm -f $(TARGET)

# Check that the qemu-plugin.h is actually found and has the right version
check-deps:
	@if [ ! -f "$(QEMU_INCLUDE)/qemu-plugin.h" ]; then \
	    echo "ERROR: $(QEMU_INCLUDE)/qemu-plugin.h not found"; \
	    echo "  Set QEMU_INCLUDE=/path/to/qemu/include"; \
	    exit 1; \
	fi
	@VER=$$(grep 'QEMU_PLUGIN_VERSION' "$(QEMU_INCLUDE)/qemu-plugin.h" \
	        | grep -o '[0-9]*' | head -1); \
	if [ "$$VER" -lt 2 ] 2>/dev/null; then \
	    echo "ERROR: plugin API v$$VER detected, need >= 2 (QEMU 8.1+)"; \
	    exit 1; \
	else \
	    echo "Plugin API v$$VER OK (need >= 2)"; \
	fi

info:
	@echo "CC            = $(CC)"
	@echo "QEMU_INCLUDE  = $(QEMU_INCLUDE)"
	@echo "GLIB_CFLAGS   = $(GLIB_CFLAGS)"
	@echo "CFLAGS        = $(CFLAGS)"
	@echo "LDFLAGS       = $(LDFLAGS)"
