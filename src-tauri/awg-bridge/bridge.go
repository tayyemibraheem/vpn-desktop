// Package main is compiled with `-buildmode=c-shared` into awgbridge.dll — the embeddable
// AmneziaWG engine TayyemVPN.exe calls into, the same role wireguard-nt.dll played for plain
// WireGuard. Unlike that integration, every exported function here is one we wrote and control
// ourselves (thin wrappers around amneziawg-go's own Device/tun API), so there's no hand-derived
// C struct layout to get subtly wrong — just plain ints and null-terminated strings.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"sync"
	"unsafe"

	"github.com/amnezia-vpn/amneziawg-go/v3/conn"
	"github.com/amnezia-vpn/amneziawg-go/v3/device"
	"github.com/amnezia-vpn/amneziawg-go/v3/tun"
)

var (
	mu      sync.Mutex
	tunnels = map[int32]*device.Device{}
	nextID  int32 = 1
)

// awgStart creates a Wintun adapter named `name`, brings up an AmneziaWG device on it, and
// returns a positive handle for use with the other exports — or a negative error code:
// -1 failed to create the TUN adapter, -2 failed to bring the device up.
//
//export awgStart
func awgStart(cName *C.char, mtu C.int) C.int {
	name := C.GoString(cName)
	tunDevice, err := tun.CreateTUN(name, int(mtu))
	if err != nil {
		return -1
	}

	logger := device.NewLogger(device.LogLevelError, "(awgbridge) ")
	dev := device.NewDevice(tunDevice, conn.NewDefaultBind(), logger)
	if err := dev.Up(); err != nil {
		dev.Close()
		return -2
	}

	mu.Lock()
	handle := nextID
	nextID++
	tunnels[handle] = dev
	mu.Unlock()
	return C.int(handle)
}

// awgConfigure applies a UAPI-format config (the same key=value protocol `wg`/`awg` themselves
// use — private_key, listen_port, jc/jmin/jmax/s1/s2/h1-h4, and one or more peer blocks with
// public_key/preshared_key/endpoint/allowed_ip, all in the exact syntax documented by
// amneziawg-go's device/uapi.go). Keys are hex, not base64 — the caller must convert. Returns 0
// on success, -1 for an unknown handle, -2 if the config was rejected.
//
//export awgConfigure
func awgConfigure(handle C.int, cConfig *C.char) C.int {
	dev, ok := lookup(handle)
	if !ok {
		return -1
	}
	if err := dev.IpcSet(C.GoString(cConfig)); err != nil {
		return -2
	}
	return 0
}

// awgStatus returns the device's current UAPI "get" dump (peer public keys, endpoints,
// handshake times, tx/rx counters) as a newly allocated C string — the caller must free it with
// awgFreeString. Returns NULL for an unknown handle.
//
//export awgStatus
func awgStatus(handle C.int) *C.char {
	dev, ok := lookup(handle)
	if !ok {
		return nil
	}
	status, err := dev.IpcGet()
	if err != nil {
		return nil
	}
	return C.CString(status)
}

//export awgFreeString
func awgFreeString(s *C.char) {
	C.free(unsafe.Pointer(s))
}

// awgStop tears down the device and its adapter. Safe to call on an already-stopped or unknown
// handle.
//
//export awgStop
func awgStop(handle C.int) {
	mu.Lock()
	dev, ok := tunnels[int32(handle)]
	if ok {
		delete(tunnels, int32(handle))
	}
	mu.Unlock()
	if ok {
		dev.Close()
	}
}

func lookup(handle C.int) (*device.Device, bool) {
	mu.Lock()
	defer mu.Unlock()
	dev, ok := tunnels[int32(handle)]
	return dev, ok
}

func main() {}
