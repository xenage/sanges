// Copyright 2019 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::io;

use crate::bus::BusDevice;
use crate::legacy::gic::GICDevice;
use crate::legacy::irqchip::IrqChipT;
use crate::Error as DeviceError;

use hvf::bindings::{hv_gic_config_t, hv_ipa_t, hv_return_t, HV_SUCCESS};
use hvf::Error;
use utils::eventfd::EventFd;

const ARCH_GIC_V3_MAINT_IRQ: u32 = 9;
const HVF_SYMBOL_MISSING: hv_return_t = -1;

unsafe extern "C" {
    fn sagens_hvf_gic_config_create(config: *mut hv_gic_config_t) -> hv_return_t;
    fn sagens_hvf_gic_config_set_distributor_base(
        config: hv_gic_config_t,
        addr: hv_ipa_t,
    ) -> hv_return_t;
    fn sagens_hvf_gic_config_set_redistributor_base(
        config: hv_gic_config_t,
        addr: hv_ipa_t,
    ) -> hv_return_t;
    fn sagens_hvf_gic_create(config: hv_gic_config_t) -> hv_return_t;
    fn sagens_hvf_gic_get_distributor_size(dist_size: *mut usize) -> hv_return_t;
    fn sagens_hvf_gic_get_redistributor_size(redist_size: *mut usize) -> hv_return_t;
    fn sagens_hvf_gic_set_spi(intid: u32, asserted: bool) -> hv_return_t;
}

pub struct HvfGicV3 {
    properties: [u64; 4],
    vcpu_count: u64,
}

impl HvfGicV3 {
    pub fn new(vcpu_count: u64) -> Result<Self, Error> {
        let mut dist_size: usize = 0;
        let ret = unsafe { sagens_hvf_gic_get_distributor_size(&mut dist_size) };
        if ret == HVF_SYMBOL_MISSING {
            return Err(Error::FindSymbol("hv_gic_get_distributor_size"));
        }
        if ret != HV_SUCCESS {
            return Err(Error::VmCreate);
        }
        let dist_size = dist_size as u64;

        let mut redist_size: usize = 0;
        let ret = unsafe { sagens_hvf_gic_get_redistributor_size(&mut redist_size) };
        if ret == HVF_SYMBOL_MISSING {
            return Err(Error::FindSymbol("hv_gic_get_redistributor_size"));
        }
        if ret != HV_SUCCESS {
            return Err(Error::VmCreate);
        }

        let redists_size = redist_size as u64 * vcpu_count;
        let dist_addr = arch::MMIO_MEM_START - dist_size - redists_size;
        let redists_addr = arch::MMIO_MEM_START - redists_size;

        let mut gic_config: hv_gic_config_t = std::ptr::null_mut();
        let ret = unsafe { sagens_hvf_gic_config_create(&mut gic_config) };
        if ret == HVF_SYMBOL_MISSING {
            return Err(Error::FindSymbol("hv_gic_config_create"));
        }
        if ret != HV_SUCCESS {
            return Err(Error::VmCreate);
        }

        let ret = unsafe {
            sagens_hvf_gic_config_set_distributor_base(gic_config, dist_addr as hv_ipa_t)
        };
        if ret == HVF_SYMBOL_MISSING {
            return Err(Error::FindSymbol("hv_gic_config_set_distributor_base"));
        }
        if ret != HV_SUCCESS {
            return Err(Error::VmCreate);
        }

        let ret = unsafe {
            sagens_hvf_gic_config_set_redistributor_base(
                gic_config,
                (arch::MMIO_MEM_START - redists_size) as hv_ipa_t,
            )
        };
        if ret == HVF_SYMBOL_MISSING {
            return Err(Error::FindSymbol("hv_gic_config_set_redistributor_base"));
        }
        if ret != HV_SUCCESS {
            return Err(Error::VmCreate);
        }

        let ret = unsafe { sagens_hvf_gic_create(gic_config) };
        if ret == HVF_SYMBOL_MISSING {
            return Err(Error::FindSymbol("hv_gic_create"));
        }
        if ret != HV_SUCCESS {
            return Err(Error::VmCreate);
        }

        Ok(Self {
            properties: [dist_addr, dist_size, redists_addr, redists_size],
            vcpu_count,
        })
    }
}

impl IrqChipT for HvfGicV3 {
    fn get_mmio_addr(&self) -> u64 {
        0
    }

    fn get_mmio_size(&self) -> u64 {
        0
    }

    fn set_irq(
        &self,
        irq_line: Option<u32>,
        _interrupt_evt: Option<&EventFd>,
    ) -> Result<(), DeviceError> {
        if let Some(irq_line) = irq_line {
            let ret = unsafe { sagens_hvf_gic_set_spi(irq_line, true) };
            if ret != HV_SUCCESS {
                Err(DeviceError::FailedSignalingUsedQueue(
                    std::io::Error::other(if ret == HVF_SYMBOL_MISSING {
                        "HVF GIC SPI symbol unavailable"
                    } else {
                        "HVF returned error when setting SPI"
                    }),
                ))
            } else {
                Ok(())
            }
        } else {
            Err(DeviceError::FailedSignalingUsedQueue(io::Error::new(
                io::ErrorKind::InvalidData,
                "IRQ not line configured",
            )))
        }
    }
}

impl BusDevice for HvfGicV3 {
    fn read(&mut self, _vcpuid: u64, _offset: u64, _data: &mut [u8]) {
        unreachable!("MMIO operations are managed in-kernel");
    }

    fn write(&mut self, _vcpuid: u64, _offset: u64, _data: &[u8]) {
        unreachable!("MMIO operations are managed in-kernel");
    }
}

impl GICDevice for HvfGicV3 {
    fn device_properties(&self) -> Vec<u64> {
        self.properties.to_vec()
    }

    fn vcpu_count(&self) -> u64 {
        self.vcpu_count
    }

    fn fdt_compatibility(&self) -> String {
        "arm,gic-v3".to_string()
    }

    fn fdt_maint_irq(&self) -> u32 {
        ARCH_GIC_V3_MAINT_IRQ
    }

    fn version(&self) -> u32 {
        7
    }
}
