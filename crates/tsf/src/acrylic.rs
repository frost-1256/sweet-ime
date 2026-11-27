use std::cell::RefCell;
use std::rc::{Rc, Weak};

use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
};
use windows::System::{DispatcherQueue, DispatcherQueueController};
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{CompositionEffectSourceParameter, Compositor};
use windows::Win32::Foundation::{E_INVALIDARG, HWND};
use windows::Win32::Graphics::Direct2D::Common::D2D1_BORDER_MODE_HARD;
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1GaussianBlur, D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    GRAPHICS_EFFECT_PROPERTY_MAPPING, IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows_core::{GUID, HSTRING, Interface, PCWSTR, Result, implement};
use windows_numerics::Vector2;

thread_local! {
    static OWNED_QUEUE: RefCell<Weak<BlurQueue>> = const { RefCell::new(Weak::new()) };
}

pub(super) struct BlurQueue(DispatcherQueueController);

impl BlurQueue {
    pub(super) fn acquire() -> Result<Option<Rc<Self>>> {
        if let Some(queue) = OWNED_QUEUE.with(|queue| queue.borrow().upgrade()) {
            return Ok(Some(queue));
        }
        if DispatcherQueue::GetForCurrentThread().is_ok() {
            // The TSF host owns this queue.
            return Ok(None);
        }
        let controller = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })?
        };
        let queue = Rc::new(Self(controller));
        OWNED_QUEUE.with(|owned| owned.replace(Rc::downgrade(&queue)));
        Ok(Some(queue))
    }
}

impl Drop for BlurQueue {
    fn drop(&mut self) {
        // The host message loop drains the queue. It must never be blocked here.
        let _ = self.0.ShutdownQueueAsync();
    }
}

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct GaussianBlur {
    name: RefCell<HSTRING>,
    source: IGraphicsEffectSource,
    deviation: f32,
}

impl IGraphicsEffectSource_Impl for GaussianBlur_Impl {}

impl IGraphicsEffect_Impl for GaussianBlur_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.borrow().clone())
    }

    fn SetName(&self, name: &HSTRING) -> Result<()> {
        self.name.replace(name.clone());
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for GaussianBlur_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }

    fn GetNamedPropertyMapping(
        &self,
        _: &PCWSTR,
        _: *mut u32,
        _: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        // All values are set by index, with no animatable properties.
        Err(E_INVALIDARG.into())
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingle(self.deviation),
            1 => PropertyValue::CreateUInt32(D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED.0 as u32),
            2 => PropertyValue::CreateUInt32(D2D1_BORDER_MODE_HARD.0 as u32),
            _ => return Err(E_INVALIDARG.into()),
        }?
        .cast()
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        match index {
            0 => Ok(self.source.clone()),
            _ => Err(E_INVALIDARG.into()),
        }
    }
}

pub(super) struct BlurComposition {
    _target: DesktopWindowTarget,
    _compositor: Compositor,
}

pub(super) fn custom_blur(hwnd: HWND, deviation: f32) -> Result<BlurComposition> {
    let enabled = 1i32;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            (&raw const enabled).cast(),
            4,
        )?;
    }
    let compositor = Compositor::new()?;
    let desktop: ICompositorDesktopInterop = compositor.cast()?;
    let target = unsafe { desktop.CreateDesktopWindowTarget(hwnd, false)? };
    let parameter = HSTRING::from("Backdrop");
    let effect: IGraphicsEffect = GaussianBlur {
        name: RefCell::new(HSTRING::new()),
        source: CompositionEffectSourceParameter::Create(&parameter)?.cast()?,
        deviation,
    }
    .into();
    let brush = compositor.CreateEffectFactory(&effect)?.CreateBrush()?;
    brush.SetSourceParameter(&parameter, &compositor.CreateBackdropBrush()?)?;
    let visual = compositor.CreateSpriteVisual()?;
    visual.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
    visual.SetBrush(&brush)?;
    target.SetRoot(&visual)?;
    Ok(BlurComposition {
        _target: target,
        _compositor: compositor,
    })
}
