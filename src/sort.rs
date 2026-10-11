use std::cmp::Ordering;

use clap::ValueEnum;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SortOrder {
    Asc,
    Desc,
}

pub enum SortValue<'a> {
    Text(&'a str),
    Number(Option<f64>),
}

pub trait Sortable {
    type Key: Copy;

    fn sort_value(&self, key: Self::Key) -> SortValue<'_>;
}

pub fn sort_rows<T: Sortable>(rows: &mut [T], key: T::Key, order: Option<SortOrder>) {
    rows.sort_by(|left, right| compare(&left.sort_value(key), &right.sort_value(key), order));
}

pub fn compare(left: &SortValue, right: &SortValue, order: Option<SortOrder>) -> Ordering {
    match (left, right) {
        (SortValue::Text(left), SortValue::Text(right)) => {
            directed(left.cmp(right), order.unwrap_or(SortOrder::Asc))
        }
        (SortValue::Number(Some(left)), SortValue::Number(Some(right))) => {
            directed(left.total_cmp(right), order.unwrap_or(SortOrder::Desc))
        }
        (SortValue::Number(Some(_)), SortValue::Number(None)) => Ordering::Less,
        (SortValue::Number(None), SortValue::Number(Some(_))) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

pub fn directed(ordering: Ordering, order: SortOrder) -> Ordering {
    match order {
        SortOrder::Asc => ordering,
        SortOrder::Desc => ordering.reverse(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Row(&'static str, Option<f64>);

    impl Sortable for Row {
        type Key = bool;

        fn sort_value(&self, numeric: bool) -> SortValue<'_> {
            if numeric {
                SortValue::Number(self.1)
            } else {
                SortValue::Text(self.0)
            }
        }
    }

    fn names(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.0).collect()
    }

    #[test]
    fn numbers_default_to_descending_and_missing_values_sort_last() {
        let mut rows = vec![Row("a", Some(1.0)), Row("b", None), Row("c", Some(3.0))];
        sort_rows(&mut rows, true, None);
        assert_eq!(names(&rows), ["c", "a", "b"]);
        sort_rows(&mut rows, true, Some(SortOrder::Asc));
        assert_eq!(names(&rows), ["a", "c", "b"]);
    }

    #[test]
    fn text_defaults_to_ascending() {
        let mut rows = vec![Row("b", None), Row("c", None), Row("a", None)];
        sort_rows(&mut rows, false, None);
        assert_eq!(names(&rows), ["a", "b", "c"]);
        sort_rows(&mut rows, false, Some(SortOrder::Desc));
        assert_eq!(names(&rows), ["c", "b", "a"]);
    }
}
