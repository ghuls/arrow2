use std::sync::Arc;

use crate::{
    bitmap::Bitmap,
    datatypes::{DataType, Field},
};

use super::{display_fmt, ffi::ToFfi, new_empty_array, new_null_array, Array};

mod iterator;
pub use iterator::*;
mod mutable;
pub use mutable::*;

#[derive(Debug, Clone)]
pub struct FixedSizeListArray {
    size: i32, // this is redundant with `data_type`, but useful to not have to deconstruct the data_type.
    data_type: DataType,
    values: Arc<dyn Array>,
    validity: Option<Bitmap>,
    offset: usize,
}

impl FixedSizeListArray {
    pub fn new_empty(data_type: DataType) -> Self {
        let values = new_empty_array(Self::get_child_and_size(&data_type).0.clone()).into();
        Self::from_data(data_type, values, None)
    }

    pub fn new_null(data_type: DataType, length: usize) -> Self {
        let values = new_null_array(Self::get_child_and_size(&data_type).0.clone(), length).into();
        Self::from_data(data_type, values, Some(Bitmap::new_zeroed(length)))
    }

    pub fn from_data(
        data_type: DataType,
        values: Arc<dyn Array>,
        validity: Option<Bitmap>,
    ) -> Self {
        let (_, size) = Self::get_child_and_size(&data_type);

        assert_eq!(values.len() % (*size as usize), 0);

        Self {
            size: *size,
            data_type,
            values,
            validity,
            offset: 0,
        }
    }

    pub fn slice(&self, offset: usize, length: usize) -> Self {
        let validity = self.validity.clone().map(|x| x.slice(offset, length));
        let values = self
            .values
            .clone()
            .slice(offset * self.size as usize, length * self.size as usize)
            .into();
        Self {
            data_type: self.data_type.clone(),
            size: self.size,
            values,
            validity,
            offset: self.offset + offset,
        }
    }

    #[inline]
    pub fn values(&self) -> &Arc<dyn Array> {
        &self.values
    }

    #[inline]
    pub fn value(&self, i: usize) -> Box<dyn Array> {
        self.values
            .slice(i * self.size as usize, self.size as usize)
    }
}

impl FixedSizeListArray {
    pub(crate) fn get_child_and_size(data_type: &DataType) -> (&DataType, &i32) {
        if let DataType::FixedSizeList(field, size) = data_type {
            (field.data_type(), size)
        } else {
            panic!("Wrong DataType")
        }
    }

    #[inline]
    pub fn default_datatype(data_type: DataType, size: usize) -> DataType {
        let field = Box::new(Field::new("item", data_type, true));
        DataType::FixedSizeList(field, size as i32)
    }
}

impl Array for FixedSizeListArray {
    #[inline]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    #[inline]
    fn len(&self) -> usize {
        self.values.len() / self.size as usize
    }

    #[inline]
    fn data_type(&self) -> &DataType {
        &self.data_type
    }

    fn validity(&self) -> &Option<Bitmap> {
        &self.validity
    }

    fn slice(&self, offset: usize, length: usize) -> Box<dyn Array> {
        Box::new(self.slice(offset, length))
    }
}

impl std::fmt::Display for FixedSizeListArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display_fmt(self.iter(), "FixedSizeListArray", f, true)
    }
}

unsafe impl ToFfi for FixedSizeListArray {
    fn buffers(&self) -> Vec<Option<std::ptr::NonNull<u8>>> {
        vec![self.validity.as_ref().map(|x| x.as_ptr())]
    }

    fn offset(&self) -> usize {
        self.offset
    }

    fn children(&self) -> Vec<Arc<dyn Array>> {
        vec![self.values().clone()]
    }
}
