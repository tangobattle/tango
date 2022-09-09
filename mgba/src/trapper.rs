use super::core;
use super::gba;

#[repr(transparent)]
pub struct Trapper(Box<TrapperCStruct>);

#[repr(C)]
struct TrapperCStruct {
    cpu_component: mgba_sys::mCPUComponent,
    real_bkpt16: Option<unsafe extern "C" fn(*mut mgba_sys::ARMCore, i32)>,
    r#impl: Impl,
}

struct Trap {
    handler: Box<dyn Fn(core::CoreMutRef)>,
    original: u16,
}

struct Impl {
    traps: std::collections::HashMap<u32, Trap>,
    core_ptr: *mut mgba_sys::mCore,
}

unsafe impl Send for TrapperCStruct {}
unsafe impl Send for Impl {}

const TRAPPER_IMM: i32 = 0xef;

unsafe extern "C" fn c_trapper_init(_cpu: *mut std::os::raw::c_void, _cpu_component: *mut mgba_sys::mCPUComponent) {}

unsafe extern "C" fn c_trapper_deinit(_cpu_component: *mut mgba_sys::mCPUComponent) {}

unsafe extern "C" fn c_trapper_bkpt16(arm_core: *mut mgba_sys::ARMCore, imm: i32) {
    let gba = gba::GBAMutRef {
        ptr: (*arm_core).master as *mut mgba_sys::GBA,
        _lifetime: std::marker::PhantomData,
    };
    let arm_core = gba.cpu_mut();
    let components = arm_core.components_mut();
    let trapper =
        &mut *(components[mgba_sys::mCPUComponentType_CPU_COMPONENT_MISC_1 as usize] as *mut _ as *mut TrapperCStruct);
    if imm == TRAPPER_IMM {
        let r#impl = &mut trapper.r#impl;
        let caller = arm_core.as_ref().gpr(15) as u32 - mgba_sys::WordSize_WORD_SIZE_THUMB * 2;
        let trap = r#impl.traps.get_mut(&caller).unwrap();
        mgba_sys::ARMRunFake(arm_core.ptr, trap.original as u32);
        let mut core = core::CoreMutRef {
            ptr: r#impl.core_ptr,
            _lifetime: std::marker::PhantomData,
        };
        (trap.handler)(core);
        core.step();
    }
    (*trapper).real_bkpt16.unwrap()(arm_core.ptr, imm);
}

impl Trapper {
    pub fn new(mut core: core::CoreMutRef, handlers: Vec<(u32, Box<dyn Fn(core::CoreMutRef)>)>) -> Self {
        let mut cpu_component = unsafe { std::mem::zeroed::<mgba_sys::mCPUComponent>() };
        cpu_component.init = Some(c_trapper_init);
        cpu_component.deinit = Some(c_trapper_deinit);
        let mut trapper_c_struct = Box::new(TrapperCStruct {
            cpu_component,
            real_bkpt16: None,
            r#impl: Impl {
                traps: std::collections::HashMap::new(),
                core_ptr: core.ptr,
            },
        });

        unsafe {
            let arm_core = &mut *core.gba_mut().cpu_mut().ptr;
            trapper_c_struct.real_bkpt16 = (*arm_core).irqh.bkpt16;
            let components = std::slice::from_raw_parts_mut(
                (*arm_core).components,
                mgba_sys::mCPUComponentType_CPU_COMPONENT_MAX as usize,
            );
            components[mgba_sys::mCPUComponentType_CPU_COMPONENT_MISC_1 as usize] =
                &mut *trapper_c_struct as *mut _ as *mut mgba_sys::mCPUComponent;
            mgba_sys::ARMHotplugAttach(
                arm_core,
                mgba_sys::mCPUComponentType_CPU_COMPONENT_MISC_1 as mgba_sys::size_t,
            );
            arm_core.irqh.bkpt16 = Some(c_trapper_bkpt16);
        }

        for (addr, handler) in handlers {
            match trapper_c_struct.r#impl.traps.entry(addr) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    panic!("attempting to install a second trap at 0x{:08x}", addr);
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    let mut original = 0i16;
                    unsafe {
                        mgba_sys::GBAPatch16(
                            core.gba_mut().cpu_mut().ptr,
                            addr,
                            (0xbe00 | TRAPPER_IMM) as i16,
                            &mut original,
                        )
                    };
                    e.insert(Trap {
                        original: original as u16,
                        handler,
                    });
                }
            };
        }
        Trapper(trapper_c_struct)
    }
}
