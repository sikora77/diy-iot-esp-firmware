use core::error::Error;
use core::fmt;

pub struct SSIDFlashError;
impl fmt::Display for SSIDFlashError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Could not read SSID from flash")
	}
}
impl fmt::Debug for SSIDFlashError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{{ file: {}, line: {} }}", file!(), line!())
	}
}
impl Error for SSIDFlashError {}
pub struct PasswordFlashError;
impl fmt::Display for PasswordFlashError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Could not read Password from flash")
	}
}
impl fmt::Debug for PasswordFlashError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{{ file: {}, line: {} }}", file!(), line!())
	}
}
impl Error for PasswordFlashError {}
