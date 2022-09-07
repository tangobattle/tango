#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ARMCoreRef<'a> {
    pub(super) ptr: *const mgba_sys::ARMCore,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> ARMCoreRef<'a> {
    pub fn gpr(&self, r: usize) -> i32 {
        unsafe { (*self.ptr).__bindgen_anon_1.__bindgen_anon_1.gprs[r] }
    }

    pub fn cpsr(&self) -> i32 {
        unsafe { (*self.ptr).__bindgen_anon_1.__bindgen_anon_1.cpsr.packed }
    }

    pub fn thumb_pc(&self) -> u32 {
        self.gpr(15) as u32 - mgba_sys::WordSize_WORD_SIZE_THUMB
    }

    pub fn arm_pc(&self) -> u32 {
        self.gpr(15) as u32 - mgba_sys::WordSize_WORD_SIZE_ARM
    }

    pub fn execution_mode(&self) -> ExecutionMode {
        unsafe {
            match (*self.ptr).executionMode {
                mgba_sys::ExecutionMode_MODE_ARM => ExecutionMode::ARM,
                mgba_sys::ExecutionMode_MODE_THUMB => ExecutionMode::Thumb,
                _ => unreachable!(),
            }
        }
    }
}

pub enum ExecutionMode {
    ARM,
    Thumb,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ARMCoreMutRef<'a> {
    pub(super) ptr: *mut mgba_sys::ARMCore,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> ARMCoreMutRef<'a> {
    pub fn as_ref(&self) -> ARMCoreRef {
        ARMCoreRef {
            ptr: self.ptr,
            _lifetime: self._lifetime,
        }
    }

    pub unsafe fn components_mut(&self) -> &[*mut mgba_sys::mCPUComponent] {
        std::slice::from_raw_parts_mut(
            (*self.ptr).components,
            mgba_sys::mCPUComponentType_CPU_COMPONENT_MAX as usize,
        )
    }

    pub fn set_gpr(&self, r: usize, v: i32) {
        unsafe {
            (*self.ptr).__bindgen_anon_1.__bindgen_anon_1.gprs[r] = v;
        }
    }

    pub fn set_thumb_pc(&self, v: u32) {
        self.set_gpr(15, v as i32);
        self.thumb_write_pc();
    }

    fn thumb_write_pc(&self) {
        unsafe {
            // uint32_t pc = cpu->gprs[ARM_PC] & -WORD_SIZE_THUMB;
            let mut pc =
                (self.as_ref().gpr(mgba_sys::ARM_PC as usize) & -(mgba_sys::WordSize_WORD_SIZE_THUMB as i32)) as u32;

            // cpu->memory.setActiveRegion(cpu, pc);
            (*self.ptr).memory.setActiveRegion.unwrap()(self.ptr, pc as u32);

            // LOAD_16(cpu->prefetch[0], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
            (*self.ptr).prefetch[0] = *(((*self.ptr).memory.activeRegion as *const u8)
                .add((pc & (*self.ptr).memory.activeMask) as usize)
                as *const u16) as u32;

            // pc += WORD_SIZE_THUMB;
            pc += mgba_sys::WordSize_WORD_SIZE_THUMB;

            // LOAD_16(cpu->prefetch[1], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
            (*self.ptr).prefetch[1] = *(((*self.ptr).memory.activeRegion as *const u8)
                .add((pc & (*self.ptr).memory.activeMask) as usize)
                as *const u16) as u32;

            // cpu->gprs[ARM_PC] = pc;
            self.set_gpr(mgba_sys::ARM_PC as usize, pc as i32);
        }
    }
}
