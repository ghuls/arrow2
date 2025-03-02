use std::cmp::Ordering;

use crate::array::ord;
use crate::compute::take;
use crate::datatypes::*;
use crate::error::{ArrowError, Result};
use crate::{
    array::*,
    types::{days_ms, NativeType},
};

use crate::buffer::MutableBuffer;

mod boolean;
mod common;
mod lex_sort;
mod primitive;
mod utf8;

pub(crate) use lex_sort::{build_compare, Compare};
pub use lex_sort::{lexsort, lexsort_to_indices, SortColumn};

macro_rules! dyn_sort {
    ($ty:ty, $array:expr, $cmp:expr, $options:expr, $limit:expr) => {{
        let array = $array
            .as_any()
            .downcast_ref::<PrimitiveArray<$ty>>()
            .unwrap();
        Ok(Box::new(primitive::sort_by::<$ty, _>(
            &array, $cmp, $options, $limit,
        )))
    }};
}

/// Sort the [`Array`] using [`SortOptions`].
///
/// Performs an unstable sort on values and indices. Nulls are ordered according to the `nulls_first` flag in `options`.
/// Floats are sorted using IEEE 754 totalOrder
/// # Errors
/// Errors if the [`DataType`] is not supported.
pub fn sort(
    values: &dyn Array,
    options: &SortOptions,
    limit: Option<usize>,
) -> Result<Box<dyn Array>> {
    match values.data_type() {
        DataType::Int8 => dyn_sort!(i8, values, ord::total_cmp, options, limit),
        DataType::Int16 => dyn_sort!(i16, values, ord::total_cmp, options, limit),
        DataType::Int32
        | DataType::Date32
        | DataType::Time32(_)
        | DataType::Interval(IntervalUnit::YearMonth) => {
            dyn_sort!(i32, values, ord::total_cmp, options, limit)
        }
        DataType::Int64
        | DataType::Date64
        | DataType::Time64(_)
        | DataType::Timestamp(_, None)
        | DataType::Duration(_) => dyn_sort!(i64, values, ord::total_cmp, options, limit),
        DataType::UInt8 => dyn_sort!(u8, values, ord::total_cmp, options, limit),
        DataType::UInt16 => dyn_sort!(u16, values, ord::total_cmp, options, limit),
        DataType::UInt32 => dyn_sort!(u32, values, ord::total_cmp, options, limit),
        DataType::UInt64 => dyn_sort!(u64, values, ord::total_cmp, options, limit),
        DataType::Float32 => dyn_sort!(f32, values, ord::total_cmp_f32, options, limit),
        DataType::Float64 => dyn_sort!(f64, values, ord::total_cmp_f64, options, limit),
        DataType::Interval(IntervalUnit::DayTime) => {
            dyn_sort!(days_ms, values, ord::total_cmp, options, limit)
        }
        _ => {
            let indices = sort_to_indices::<u64>(values, options, limit)?;
            take::take(values, &indices)
        }
    }
}

// partition indices into valid and null indices
fn partition_validity<I: Index>(array: &dyn Array) -> (Vec<I>, Vec<I>) {
    let length = array.len();
    let indices = (0..length).map(|x| I::from_usize(x).unwrap());
    if let Some(validity) = array.validity() {
        indices.partition(|index| validity.get_bit(index.to_usize()))
    } else {
        (indices.collect(), vec![])
    }
}

macro_rules! dyn_sort_indices {
    ($index:ty, $ty:ty, $array:expr, $cmp:expr, $options:expr, $limit:expr) => {{
        let array = $array
            .as_any()
            .downcast_ref::<PrimitiveArray<$ty>>()
            .unwrap();
        Ok(primitive::indices_sorted_unstable_by::<$index, $ty, _>(
            &array, $cmp, $options, $limit,
        ))
    }};
}

/// Sort elements from `values` into a non-nullable [`PrimitiveArray`] of indices that sort `values`.
pub fn sort_to_indices<I: Index>(
    values: &dyn Array,
    options: &SortOptions,
    limit: Option<usize>,
) -> Result<PrimitiveArray<I>> {
    match values.data_type() {
        DataType::Boolean => {
            let (v, n) = partition_validity(values);
            Ok(boolean::sort_boolean(
                values.as_any().downcast_ref().unwrap(),
                v,
                n,
                options,
                limit,
            ))
        }
        DataType::Int8 => dyn_sort_indices!(I, i8, values, ord::total_cmp, options, limit),
        DataType::Int16 => dyn_sort_indices!(I, i16, values, ord::total_cmp, options, limit),
        DataType::Int32
        | DataType::Date32
        | DataType::Time32(_)
        | DataType::Interval(IntervalUnit::YearMonth) => {
            dyn_sort_indices!(I, i32, values, ord::total_cmp, options, limit)
        }
        DataType::Int64
        | DataType::Date64
        | DataType::Time64(_)
        | DataType::Timestamp(_, None)
        | DataType::Duration(_) => {
            dyn_sort_indices!(I, i64, values, ord::total_cmp, options, limit)
        }
        DataType::UInt8 => dyn_sort_indices!(I, u8, values, ord::total_cmp, options, limit),
        DataType::UInt16 => dyn_sort_indices!(I, u16, values, ord::total_cmp, options, limit),
        DataType::UInt32 => dyn_sort_indices!(I, u32, values, ord::total_cmp, options, limit),
        DataType::UInt64 => dyn_sort_indices!(I, u64, values, ord::total_cmp, options, limit),
        DataType::Float32 => dyn_sort_indices!(I, f32, values, ord::total_cmp_f32, options, limit),
        DataType::Float64 => dyn_sort_indices!(I, f64, values, ord::total_cmp_f64, options, limit),
        DataType::Interval(IntervalUnit::DayTime) => {
            dyn_sort_indices!(I, days_ms, values, ord::total_cmp, options, limit)
        }
        DataType::Utf8 => Ok(utf8::indices_sorted_unstable_by::<I, i32>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::LargeUtf8 => Ok(utf8::indices_sorted_unstable_by::<I, i64>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::List(field) => {
            let (v, n) = partition_validity(values);
            match field.data_type() {
                DataType::Int8 => Ok(sort_list::<I, i32, i8>(values, v, n, options, limit)),
                DataType::Int16 => Ok(sort_list::<I, i32, i16>(values, v, n, options, limit)),
                DataType::Int32 => Ok(sort_list::<I, i32, i32>(values, v, n, options, limit)),
                DataType::Int64 => Ok(sort_list::<I, i32, i64>(values, v, n, options, limit)),
                DataType::UInt8 => Ok(sort_list::<I, i32, u8>(values, v, n, options, limit)),
                DataType::UInt16 => Ok(sort_list::<I, i32, u16>(values, v, n, options, limit)),
                DataType::UInt32 => Ok(sort_list::<I, i32, u32>(values, v, n, options, limit)),
                DataType::UInt64 => Ok(sort_list::<I, i32, u64>(values, v, n, options, limit)),
                t => Err(ArrowError::NotYetImplemented(format!(
                    "Sort not supported for list type {:?}",
                    t
                ))),
            }
        }
        DataType::LargeList(field) => {
            let (v, n) = partition_validity(values);
            match field.data_type() {
                DataType::Int8 => Ok(sort_list::<I, i64, i8>(values, v, n, options, limit)),
                DataType::Int16 => Ok(sort_list::<I, i64, i16>(values, v, n, options, limit)),
                DataType::Int32 => Ok(sort_list::<I, i64, i32>(values, v, n, options, limit)),
                DataType::Int64 => Ok(sort_list::<I, i64, i64>(values, v, n, options, limit)),
                DataType::UInt8 => Ok(sort_list::<I, i64, u8>(values, v, n, options, limit)),
                DataType::UInt16 => Ok(sort_list::<I, i64, u16>(values, v, n, options, limit)),
                DataType::UInt32 => Ok(sort_list::<I, i64, u32>(values, v, n, options, limit)),
                DataType::UInt64 => Ok(sort_list::<I, i64, u64>(values, v, n, options, limit)),
                t => Err(ArrowError::NotYetImplemented(format!(
                    "Sort not supported for list type {:?}",
                    t
                ))),
            }
        }
        DataType::FixedSizeList(field, _) => {
            let (v, n) = partition_validity(values);
            match field.data_type() {
                DataType::Int8 => Ok(sort_list::<I, i32, i8>(values, v, n, options, limit)),
                DataType::Int16 => Ok(sort_list::<I, i32, i16>(values, v, n, options, limit)),
                DataType::Int32 => Ok(sort_list::<I, i32, i32>(values, v, n, options, limit)),
                DataType::Int64 => Ok(sort_list::<I, i32, i64>(values, v, n, options, limit)),
                DataType::UInt8 => Ok(sort_list::<I, i32, u8>(values, v, n, options, limit)),
                DataType::UInt16 => Ok(sort_list::<I, i32, u16>(values, v, n, options, limit)),
                DataType::UInt32 => Ok(sort_list::<I, i32, u32>(values, v, n, options, limit)),
                DataType::UInt64 => Ok(sort_list::<I, i32, u64>(values, v, n, options, limit)),
                t => Err(ArrowError::NotYetImplemented(format!(
                    "Sort not supported for list type {:?}",
                    t
                ))),
            }
        }
        DataType::Dictionary(key_type, value_type) => match value_type.as_ref() {
            DataType::Utf8 => sort_dict::<I, i32>(values, key_type.as_ref(), options, limit),
            DataType::LargeUtf8 => sort_dict::<I, i64>(values, key_type.as_ref(), options, limit),
            t => Err(ArrowError::NotYetImplemented(format!(
                "Sort not supported for dictionary type with keys {:?}",
                t
            ))),
        },
        t => Err(ArrowError::NotYetImplemented(format!(
            "Sort not supported for data type {:?}",
            t
        ))),
    }
}

fn sort_dict<I: Index, O: Offset>(
    values: &dyn Array,
    key_type: &DataType,
    options: &SortOptions,
    limit: Option<usize>,
) -> Result<PrimitiveArray<I>> {
    match key_type {
        DataType::Int8 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, i8, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::Int16 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, i16, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::Int32 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, i32, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::Int64 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, i64, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::UInt8 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, u8, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::UInt16 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, u16, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::UInt32 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, u32, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        DataType::UInt64 => Ok(utf8::indices_sorted_unstable_by_dictionary::<I, u64, O>(
            values.as_any().downcast_ref().unwrap(),
            options,
            limit,
        )),
        t => Err(ArrowError::NotYetImplemented(format!(
            "Sort not supported for dictionary key type {:?}",
            t
        ))),
    }
}

/// Checks if an array of type `datatype` can be sorted
///
/// # Examples
/// ```
/// use arrow2::compute::sort::can_sort;
/// use arrow2::datatypes::{DataType};
///
/// let data_type = DataType::Int8;
/// assert_eq!(can_sort(&data_type), true);
///
/// let data_type = DataType::LargeBinary;
/// assert_eq!(can_sort(&data_type), false)
/// ```
pub fn can_sort(data_type: &DataType) -> bool {
    match data_type {
        DataType::Boolean
        | DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Date32
        | DataType::Time32(_)
        | DataType::Interval(_)
        | DataType::Int64
        | DataType::Date64
        | DataType::Time64(_)
        | DataType::Timestamp(_, None)
        | DataType::Duration(_)
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Float32
        | DataType::Float64
        | DataType::Utf8
        | DataType::LargeUtf8 => true,
        DataType::List(field) | DataType::LargeList(field) | DataType::FixedSizeList(field, _) => {
            matches!(
                field.data_type(),
                DataType::Int8
                    | DataType::Int16
                    | DataType::Int32
                    | DataType::Int64
                    | DataType::UInt8
                    | DataType::UInt16
                    | DataType::UInt32
                    | DataType::UInt64
            )
        }
        DataType::Dictionary(key_type, value_type) if *value_type.as_ref() == DataType::Utf8 => {
            matches!(
                key_type.as_ref(),
                DataType::Int8
                    | DataType::Int16
                    | DataType::Int32
                    | DataType::Int64
                    | DataType::UInt8
                    | DataType::UInt16
                    | DataType::UInt32
                    | DataType::UInt64
            )
        }
        _ => false,
    }
}

/// Options that define how sort kernels should behave
#[derive(Clone, Copy, Debug)]
pub struct SortOptions {
    /// Whether to sort in descending order
    pub descending: bool,
    /// Whether to sort nulls first
    pub nulls_first: bool,
}

impl Default for SortOptions {
    fn default() -> Self {
        Self {
            descending: false,
            // default to nulls first to match spark's behavior
            nulls_first: true,
        }
    }
}

fn sort_list<I, O, T>(
    values: &dyn Array,
    value_indices: Vec<I>,
    null_indices: Vec<I>,
    options: &SortOptions,
    limit: Option<usize>,
) -> PrimitiveArray<I>
where
    I: Index,
    O: Offset,
    T: NativeType + std::cmp::PartialOrd,
{
    let mut valids: Vec<(I, Box<dyn Array>)> = values
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .map_or_else(
            || {
                let values = values.as_any().downcast_ref::<ListArray<O>>().unwrap();
                value_indices
                    .iter()
                    .copied()
                    .map(|index| (index, values.value(index.to_usize())))
                    .collect()
            },
            |values| {
                value_indices
                    .iter()
                    .copied()
                    .map(|index| (index, values.value(index.to_usize())))
                    .collect()
            },
        );

    if !options.descending {
        valids.sort_by(|a, b| cmp_array(a.1.as_ref(), b.1.as_ref()))
    } else {
        valids.sort_by(|a, b| cmp_array(a.1.as_ref(), b.1.as_ref()).reverse())
    }

    let values = valids.iter().map(|tuple| tuple.0);

    let mut values = if options.nulls_first {
        let mut buffer = MutableBuffer::<I>::from_trusted_len_iter(null_indices.into_iter());
        buffer.extend(values);
        buffer
    } else {
        let mut buffer = MutableBuffer::<I>::from_trusted_len_iter(values);
        buffer.extend(null_indices);
        buffer
    };

    values.truncate(limit.unwrap_or_else(|| values.len()));

    PrimitiveArray::<I>::from_data(I::DATA_TYPE, values.into(), None)
}

/// Compare two `Array`s based on the ordering defined in [ord](crate::array::ord).
fn cmp_array(a: &dyn Array, b: &dyn Array) -> Ordering {
    let cmp_op = ord::build_compare(a, b).unwrap();
    let length = a.len().max(b.len());

    for i in 0..length {
        let result = cmp_op(i, i);
        if result != Ordering::Equal {
            return result;
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sort_to_indices_boolean_arrays(
        data: &[Option<bool>],
        options: SortOptions,
        expected_data: &[i32],
    ) {
        let output = BooleanArray::from(data);
        let expected = Int32Array::from_slice(expected_data);
        let output = sort_to_indices(&output, &options, None).unwrap();
        assert_eq!(output, expected)
    }

    fn test_sort_primitive_arrays<T>(
        data: &[Option<T>],
        data_type: DataType,
        options: SortOptions,
        expected_data: &[Option<T>],
    ) where
        T: NativeType,
    {
        let input = PrimitiveArray::<T>::from(data).to(data_type.clone());
        let expected = PrimitiveArray::<T>::from(expected_data).to(data_type);
        let output = sort(&input, &options, None).unwrap();
        assert_eq!(expected, output.as_ref())
    }

    fn test_sort_to_indices_string_arrays(
        data: &[Option<&str>],
        options: SortOptions,
        expected_data: &[i32],
    ) {
        let input = Utf8Array::<i32>::from(&data.to_vec());
        let expected = Int32Array::from_slice(expected_data);
        let output = sort_to_indices(&input, &options, None).unwrap();
        assert_eq!(output, expected)
    }

    fn test_sort_string_arrays(
        data: &[Option<&str>],
        options: SortOptions,
        expected_data: &[Option<&str>],
    ) {
        let input = Utf8Array::<i32>::from(&data.to_vec());
        let expected = Utf8Array::<i32>::from(&expected_data.to_vec());
        let output = sort(&input, &options, None).unwrap();
        assert_eq!(expected, output.as_ref())
    }

    fn test_sort_string_dict_arrays<K: DictionaryKey>(
        data: &[Option<&str>],
        options: SortOptions,
        expected_data: &[Option<&str>],
    ) {
        let mut input = MutableDictionaryArray::<i32, MutableUtf8Array<i32>>::new();
        input.try_extend(data.iter().copied()).unwrap();
        let input = input.into_arc();

        let mut expected = MutableDictionaryArray::<i32, MutableUtf8Array<i32>>::new();
        expected.try_extend(expected_data.iter().copied()).unwrap();
        let expected = expected.into_arc();

        let output = sort(input.as_ref(), &options, None).unwrap();
        assert_eq!(expected.as_ref(), output.as_ref())
    }

    /*
    fn test_sort_list_arrays<T>(
        data: Vec<Option<Vec<Option<T::Native>>>>,
        options: Option<SortOptions>,
        expected_data: Vec<Option<Vec<Option<T::Native>>>>,
        fixed_length: Option<i32>,
    ) where
        T: ArrowPrimitiveType,
        PrimitiveArray<T>: From<Vec<Option<T::Native>>>,
    {
        // for FixedSizedList
        if let Some(length) = fixed_length {
            let input = Arc::new(build_fixed_size_list_nullable(data.clone(), length));
            let sorted = sort(&(input as ArrayRef), options).unwrap();
            let expected = Arc::new(build_fixed_size_list_nullable(
                expected_data.clone(),
                length,
            )) as ArrayRef;

            assert_eq!(&sorted, &expected);
        }

        // for List
        let input = Arc::new(build_generic_list_nullable::<i32, T>(data.clone()));
        let sorted = sort(&(input as ArrayRef), options).unwrap();
        let expected =
            Arc::new(build_generic_list_nullable::<i32, T>(expected_data.clone()))
                as ArrayRef;

        assert_eq!(&sorted, &expected);

        // for LargeList
        let input = Arc::new(build_generic_list_nullable::<i64, T>(data));
        let sorted = sort(&(input as ArrayRef), options).unwrap();
        let expected =
            Arc::new(build_generic_list_nullable::<i64, T>(expected_data)) as ArrayRef;

        assert_eq!(&sorted, &expected);
    }

    fn test_lex_sort_arrays(input: Vec<SortColumn>, expected_output: Vec<ArrayRef>) {
        let sorted = lexsort(&input).unwrap();

        for (result, expected) in sorted.iter().zip(expected_output.iter()) {
            assert_eq!(result, expected);
        }
    }
    */

    #[test]
    fn test_sort_boolean() {
        // boolean
        test_sort_to_indices_boolean_arrays(
            &[None, Some(false), Some(true), Some(true), Some(false), None],
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            &[0, 5, 1, 4, 2, 3],
        );

        // boolean, descending
        test_sort_to_indices_boolean_arrays(
            &[None, Some(false), Some(true), Some(true), Some(false), None],
            SortOptions {
                descending: true,
                nulls_first: false,
            },
            &[2, 3, 1, 4, 5, 0],
        );

        // boolean, descending, nulls first
        test_sort_to_indices_boolean_arrays(
            &[None, Some(false), Some(true), Some(true), Some(false), None],
            SortOptions {
                descending: true,
                nulls_first: true,
            },
            &[5, 0, 2, 3, 1, 4],
        );
    }

    #[test]
    #[ignore] // improve equality for NaN values. These are right but the equality fails
    fn test_nans() {
        test_sort_primitive_arrays::<f64>(
            &[None, Some(0.0), Some(2.0), Some(-1.0), Some(f64::NAN), None],
            DataType::Float64,
            SortOptions {
                descending: true,
                nulls_first: true,
            },
            &[None, None, Some(f64::NAN), Some(2.0), Some(0.0), Some(-1.0)],
        );
        test_sort_primitive_arrays::<f64>(
            &[Some(f64::NAN), Some(f64::NAN), Some(f64::NAN), Some(1.0)],
            DataType::Float64,
            SortOptions {
                descending: true,
                nulls_first: true,
            },
            &[Some(f64::NAN), Some(f64::NAN), Some(f64::NAN), Some(1.0)],
        );

        test_sort_primitive_arrays::<f64>(
            &[None, Some(0.0), Some(2.0), Some(-1.0), Some(f64::NAN), None],
            DataType::Float64,
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            &[None, None, Some(-1.0), Some(0.0), Some(2.0), Some(f64::NAN)],
        );
        // nans
        test_sort_primitive_arrays::<f64>(
            &[Some(f64::NAN), Some(f64::NAN), Some(f64::NAN), Some(1.0)],
            DataType::Float64,
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            &[Some(1.0), Some(f64::NAN), Some(f64::NAN), Some(f64::NAN)],
        );
    }

    #[test]
    fn test_sort_to_indices_strings() {
        test_sort_to_indices_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            // &[3, 0, 5, 1, 4, 2] is also valid
            &[0, 3, 5, 1, 4, 2],
        );

        test_sort_to_indices_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: true,
                nulls_first: false,
            },
            // &[2, 4, 1, 5, 3, 0] is also valid
            &[2, 4, 1, 5, 0, 3],
        );

        test_sort_to_indices_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            // &[3, 0, 5, 1, 4, 2] is also valid
            &[0, 3, 5, 1, 4, 2],
        );

        test_sort_to_indices_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: true,
                nulls_first: true,
            },
            // &[3, 0, 2, 4, 1, 5] is also valid
            &[0, 3, 2, 4, 1, 5],
        );
    }

    #[test]
    fn test_sort_strings() {
        test_sort_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            &[
                None,
                None,
                Some("-ad"),
                Some("bad"),
                Some("glad"),
                Some("sad"),
            ],
        );

        test_sort_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: true,
                nulls_first: false,
            },
            &[
                Some("sad"),
                Some("glad"),
                Some("bad"),
                Some("-ad"),
                None,
                None,
            ],
        );

        test_sort_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            &[
                None,
                None,
                Some("-ad"),
                Some("bad"),
                Some("glad"),
                Some("sad"),
            ],
        );

        test_sort_string_arrays(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: true,
                nulls_first: true,
            },
            &[
                None,
                None,
                Some("sad"),
                Some("glad"),
                Some("bad"),
                Some("-ad"),
            ],
        );
    }

    #[test]
    fn test_sort_string_dicts() {
        test_sort_string_dict_arrays::<i8>(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            &[
                None,
                None,
                Some("-ad"),
                Some("bad"),
                Some("glad"),
                Some("sad"),
            ],
        );

        test_sort_string_dict_arrays::<i16>(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: true,
                nulls_first: false,
            },
            &[
                Some("sad"),
                Some("glad"),
                Some("bad"),
                Some("-ad"),
                None,
                None,
            ],
        );

        test_sort_string_dict_arrays::<i32>(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: false,
                nulls_first: true,
            },
            &[
                None,
                None,
                Some("-ad"),
                Some("bad"),
                Some("glad"),
                Some("sad"),
            ],
        );

        test_sort_string_dict_arrays::<i16>(
            &[
                None,
                Some("bad"),
                Some("sad"),
                None,
                Some("glad"),
                Some("-ad"),
            ],
            SortOptions {
                descending: true,
                nulls_first: true,
            },
            &[
                None,
                None,
                Some("sad"),
                Some("glad"),
                Some("bad"),
                Some("-ad"),
            ],
        );
    }

    /*
    #[test]
    fn test_sort_list() {
        test_sort_list_arrays::<i8>(
            vec![
                Some(vec![Some(1)]),
                Some(vec![Some(4)]),
                Some(vec![Some(2)]),
                Some(vec![Some(3)]),
            ],
            Some(SortOptions {
                descending: false,
                nulls_first: false,
            }),
            vec![
                Some(vec![Some(1)]),
                Some(vec![Some(2)]),
                Some(vec![Some(3)]),
                Some(vec![Some(4)]),
            ],
            Some(1),
        );

        test_sort_list_arrays::<i32>(
            vec![
                Some(vec![Some(1), Some(0)]),
                Some(vec![Some(4), Some(3), Some(2), Some(1)]),
                Some(vec![Some(2), Some(3), Some(4)]),
                Some(vec![Some(3), Some(3), Some(3), Some(3)]),
                Some(vec![Some(1), Some(1)]),
            ],
            Some(SortOptions {
                descending: false,
                nulls_first: false,
            }),
            vec![
                Some(vec![Some(1), Some(0)]),
                Some(vec![Some(1), Some(1)]),
                Some(vec![Some(2), Some(3), Some(4)]),
                Some(vec![Some(3), Some(3), Some(3), Some(3)]),
                Some(vec![Some(4), Some(3), Some(2), Some(1)]),
            ],
            None,
        );

        test_sort_list_arrays::<i32>(
            vec![
                None,
                Some(vec![Some(4), None, Some(2)]),
                Some(vec![Some(2), Some(3), Some(4)]),
                None,
                Some(vec![Some(3), Some(3), None]),
            ],
            Some(SortOptions {
                descending: false,
                nulls_first: false,
            }),
            vec![
                Some(vec![Some(2), Some(3), Some(4)]),
                Some(vec![Some(3), Some(3), None]),
                Some(vec![Some(4), None, Some(2)]),
                None,
                None,
            ],
            Some(3),
        );
    }

    #[test]
    fn test_lex_sort_single_column() {
        let input = vec![SortColumn {
            values: Arc::new(PrimitiveArray::<Int64Type>::from(vec![
                Some(17),
                Some(2),
                Some(-1),
                Some(0),
            ])) as ArrayRef,
            options: None,
        }];
        let expected = vec![Arc::new(PrimitiveArray::<Int64Type>::from(vec![
            Some(-1),
            Some(0),
            Some(2),
            Some(17),
        ])) as ArrayRef];
        test_lex_sort_arrays(input, expected);
    }

    #[test]
    fn test_lex_sort_unaligned_rows() {
        let input = vec![
            SortColumn {
                values: Arc::new(PrimitiveArray::<Int64Type>::from(vec![None, Some(-1)]))
                    as ArrayRef,
                options: None,
            },
            SortColumn {
                values: Arc::new(StringArray::from(vec![Some("foo")])) as ArrayRef,
                options: None,
            },
        ];
        assert!(
            lexsort(&input).is_err(),
            "lexsort should reject columns with different row counts"
        );
    }
    */

    #[test]
    fn consistency() {
        use crate::array::new_null_array;
        use crate::datatypes::DataType::*;
        use crate::datatypes::TimeUnit;

        let datatypes = vec![
            Null,
            Boolean,
            UInt8,
            UInt16,
            UInt32,
            UInt64,
            Int8,
            Int16,
            Int32,
            Int64,
            Float32,
            Float64,
            Timestamp(TimeUnit::Second, None),
            Timestamp(TimeUnit::Millisecond, None),
            Timestamp(TimeUnit::Microsecond, None),
            Timestamp(TimeUnit::Nanosecond, None),
            Time64(TimeUnit::Microsecond),
            Time64(TimeUnit::Nanosecond),
            Date32,
            Time32(TimeUnit::Second),
            Time32(TimeUnit::Millisecond),
            Date64,
            Utf8,
            LargeUtf8,
            Binary,
            LargeBinary,
            Duration(TimeUnit::Second),
            Duration(TimeUnit::Millisecond),
            Duration(TimeUnit::Microsecond),
            Duration(TimeUnit::Nanosecond),
        ];

        datatypes.into_iter().for_each(|d1| {
            let array = new_null_array(d1.clone(), 10);
            let options = SortOptions {
                descending: true,
                nulls_first: true,
            };
            if can_sort(&d1) {
                assert!(sort(array.as_ref(), &options, None).is_ok());
            } else {
                assert!(sort(array.as_ref(), &options, None).is_err());
            }
        });
    }
}
