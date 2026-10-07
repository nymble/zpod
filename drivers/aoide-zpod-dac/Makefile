# Out-of-tree build for aoide-zpod-dac.ko.
# Standalone:  make KDIR=/lib/modules/<kver>/build
# DKMS calls kbuild directly via dkms.conf MAKE[0].
obj-m := aoide-zpod-dac.o

KDIR ?= /lib/modules/$(shell uname -r)/build
PWD  := $(shell pwd)

all:
	$(MAKE) -C $(KDIR) M=$(PWD) modules

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean
