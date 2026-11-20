#!/usr/bin/env python3
"""Fake /sys/bus/usb/devices tree for the USB Lab simulator (ZPOD_USB_SYSFS):
a USB GPS (PL2303), an RT5370 Wi-Fi adapter, a keyboard + storage combo
(the shape of a keystroke injector) and a hub. Usage: fake-usb.py <dir>"""
import os, struct, sys

KB = bytes([0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01, 0x05,
            0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x65, 0x05, 0x07, 0x19, 0x00, 0x29, 0x65, 0x81, 0x00, 0xC0])

def dev_desc(vid, pid, cls=0, bcd=0x0200):
    return bytes([18, 1]) + struct.pack('<H', bcd) + bytes([cls, 0, 0, 64]) + struct.pack('<HHH', vid, pid, 0x0100) + bytes([1, 2, 3, 1])

def cfg(ifaces, ma=100):
    body = b''.join(ifaces)
    return bytes([9, 2]) + struct.pack('<H', 9 + len(body)) + bytes([sum(1 for _ in ifaces), 1, 0, 0x80, ma // 2]) + body

def iface(n, c, s, p, eps, extra=b''):
    return bytes([9, 4, n, 0, len(eps), c, s, p, 0]) + extra + b''.join(eps)

def ep(addr, attr, size, interval=0):
    return bytes([7, 5, addr, attr]) + struct.pack('<H', size) + bytes([interval])

def write(root, name, attrs, desc=None, ifs=()):
    d = os.path.join(root, name); os.makedirs(d, exist_ok=True)
    for k, v in attrs.items():
        open(os.path.join(d, k), 'w').write(v + '\n')
    if desc:
        open(os.path.join(d, 'descriptors'), 'wb').write(desc)
    for (iname, ia, hid) in ifs:
        i = os.path.join(root, iname); os.makedirs(i, exist_ok=True)
        for k, v in ia.items():
            open(os.path.join(i, k), 'w').write(v + '\n')
        if hid:
            h = os.path.join(i, hid[0]); os.makedirs(h, exist_ok=True)
            open(os.path.join(h, 'report_descriptor'), 'wb').write(hid[1])

def main(root):
    os.makedirs(os.path.join(root, 'usb1'), exist_ok=True)
    write(root, '1-1', {'idVendor': '1a40', 'idProduct': '0101', 'bDeviceClass': '09', 'busnum': '1', 'devnum': '2', 'speed': '480', 'version': ' 2.00', 'product': 'USB 2.0 Hub'},
          dev_desc(0x1a40, 0x0101, 9) + cfg([iface(0, 9, 0, 0, [ep(0x81, 3, 1, 12)])]),
          [('1-1:1.0', {'bInterfaceClass': '09', 'bInterfaceSubClass': '00', 'bInterfaceProtocol': '00', 'bInterfaceNumber': '00'}, None)])
    write(root, '1-1.1', {'idVendor': '067b', 'idProduct': '2303', 'bDeviceClass': '00', 'busnum': '1', 'devnum': '3', 'speed': '12', 'version': ' 1.10', 'manufacturer': 'Prolific Technology Inc.', 'product': 'USB-Serial Controller'},
          dev_desc(0x067b, 0x2303, 0, 0x0110) + cfg([iface(0, 0xff, 0, 0, [ep(0x81, 3, 10, 1), ep(0x02, 2, 64), ep(0x83, 2, 64)])]),
          [('1-1.1:1.0', {'bInterfaceClass': 'ff', 'bInterfaceSubClass': '00', 'bInterfaceProtocol': '00', 'bInterfaceNumber': '00'}, None)])
    write(root, '1-1.2', {'idVendor': '148f', 'idProduct': '5370', 'bDeviceClass': '00', 'busnum': '1', 'devnum': '4', 'speed': '480', 'version': ' 2.00', 'manufacturer': 'Ralink', 'product': '802.11 n WLAN'},
          dev_desc(0x148f, 0x5370) + cfg([iface(0, 0xff, 0xff, 0xff, [ep(0x81, 2, 512), ep(0x01, 2, 512), ep(0x02, 2, 512)])], 450),
          [('1-1.2:1.0', {'bInterfaceClass': 'ff', 'bInterfaceSubClass': 'ff', 'bInterfaceProtocol': 'ff', 'bInterfaceNumber': '00'}, None)])
    hid = bytes([9, 0x21, 0x11, 0x01, 0, 1, 0x22]) + struct.pack('<H', len(KB))
    write(root, '1-1.3', {'idVendor': '1234', 'idProduct': '5678', 'bDeviceClass': '00', 'busnum': '1', 'devnum': '5', 'speed': '12', 'version': ' 2.00', 'manufacturer': 'Generic', 'product': 'USB Disk'},
          dev_desc(0x1234, 0x5678) + cfg([iface(0, 3, 1, 1, [ep(0x81, 3, 8, 10)], hid), iface(1, 8, 6, 0x50, [ep(0x82, 2, 64), ep(0x02, 2, 64)])]),
          [('1-1.3:1.0', {'bInterfaceClass': '03', 'bInterfaceSubClass': '01', 'bInterfaceProtocol': '01', 'bInterfaceNumber': '00'}, ('0003:1234:5678.0001', KB)),
           ('1-1.3:1.1', {'bInterfaceClass': '08', 'bInterfaceSubClass': '06', 'bInterfaceProtocol': '50', 'bInterfaceNumber': '01'}, None)])

main(sys.argv[1])
