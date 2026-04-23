use std::future::Future;

use pyo3::prelude::*;

use crate::error::runtime_error;

pub fn block_on<T>(future: impl Future<Output = T>) -> PyResult<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(runtime_error)?;
    Ok(runtime.block_on(future))
}
