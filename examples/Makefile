# Builds the C and C++ examples against the static library.
#
#   cargo build -p mx-remote-ffi     # or: --release, with PROFILE=release
#   make -C examples
#
# The archive is the point: it links into a C or C++ program with no runtime to
# start and nothing to install.

PROFILE ?= debug
ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))..)
LIB := $(ROOT)/target/$(PROFILE)/libmx_remote_ffi.a

WARNINGS := -Wall -Wextra -pedantic
CFLAGS ?= -std=c99 $(WARNINGS)
CXXFLAGS ?= -std=c++11 $(WARNINGS)
CPPFLAGS += -I$(ROOT)/include

# What a Rust staticlib needs from the platform.
LDLIBS := $(LIB) -lpthread -ldl -lm

BINARIES := c/discover cpp/discover

all: $(BINARIES)

c/discover: c/discover.c $(LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) -o $@ $< $(LDLIBS)

cpp/discover: cpp/discover.cpp $(LIB)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -o $@ $< $(LDLIBS)

$(LIB):
	@echo "$(LIB) is missing: run cargo build -p mx-remote-ffi first" >&2
	@false

clean:
	rm -f $(BINARIES)

.PHONY: all clean
