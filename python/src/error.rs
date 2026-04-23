use std::fmt::Display;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

pub fn runtime_error(error: impl Display) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}
