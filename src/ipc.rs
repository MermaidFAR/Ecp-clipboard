#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        CreateEventW, EVENT_MODIFY_STATE, OpenEventW, SetEvent, WaitForSingleObject,
    };
    use windows::core::{PCWSTR, w};

    #[derive(Clone, Copy)]
    pub enum Signal {
        Show,
        Hide,
        Reload,
        History,
    }

    fn name(signal: Signal) -> PCWSTR {
        match signal {
            Signal::Show => w!("Local\\EcpClipboard.Ui.Show"),
            Signal::Hide => w!("Local\\EcpClipboard.Ui.Hide"),
            Signal::Reload => w!("Local\\EcpClipboard.Background.Reload"),
            Signal::History => w!("Local\\EcpClipboard.Ui.History"),
        }
    }

    pub struct NamedEvent(HANDLE);

    impl NamedEvent {
        pub fn create(signal: Signal) -> windows::core::Result<Self> {
            unsafe { CreateEventW(None, false, false, name(signal)).map(Self) }
        }

        pub fn take(&self) -> bool {
            unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
        }

        pub fn wait(&self) {
            unsafe {
                WaitForSingleObject(self.0, u32::MAX);
            }
        }
    }

    impl Drop for NamedEvent {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn signal(signal: Signal) -> bool {
        unsafe {
            let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, name(signal)) else {
                return false;
            };
            let success = SetEvent(event).is_ok();
            let _ = CloseHandle(event);
            success
        }
    }
}

#[cfg(windows)]
pub use imp::{NamedEvent, Signal, signal};
