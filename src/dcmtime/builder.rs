use super::ZonedTime;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[must_use = "a builder does nothing until `build` is called"]
pub struct ZonedTimeBuilder {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    nanosecond: i64,
    offset: i64,
}
impl ZonedTime {
    pub fn builder(year: i64, month: i64, day: i64) -> ZonedTimeBuilder {
        ZonedTimeBuilder {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
            nanosecond: 0,
            offset: 0,
        }
    }
}
impl ZonedTimeBuilder {
    pub fn time(mut self, hour: i64, minute: i64, second: i64) -> Self {
        self.hour = hour;
        self.minute = minute;
        self.second = second;
        self
    }
    pub fn nanosecond(mut self, nanosecond: i64) -> Self {
        self.nanosecond = nanosecond;
        self
    }
    pub fn offset(mut self, seconds_east: i64) -> Self {
        self.offset = seconds_east;
        self
    }
    pub fn build(self) -> ZonedTime {
        ZonedTime::from_fields(
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.nanosecond,
            self.offset,
        )
    }
}
impl From<ZonedTimeBuilder> for ZonedTime {
    fn from(builder: ZonedTimeBuilder) -> Self {
        builder.build()
    }
}
