//! Tracer module
pub struct Tracer {
    name: String,
    start: std::time::Instant,
    events: Vec<(std::time::Instant, String)>,
}

impl Tracer {
    pub fn new<T: ToString>(name: T) -> Self {
        Tracer {
            name: name.to_string(),
            start: std::time::Instant::now(),
            events: Vec::new(),
        }
    }
    pub fn mark<T: ToString>(&mut self, event_name: T) {
        let now = std::time::Instant::now();
        self.events.push((now, event_name.to_string()));
    }

    pub fn print(&self) {
        let elapsed = self.start.elapsed();
        println!("Tracer [{}]:", self.name);

        // elapsed ms for each event since start
        let times_ms: Vec<u128> = self
            .events
            .iter()
            .map(|(time, _)| (*time - self.start).as_millis())
            .collect();

        // delta ms between consecutive events (first delta = first event elapsed)
        let deltas_ms: Vec<u128> = times_ms
            .iter()
            .enumerate()
            .map(|(i, &t)| if i == 0 { t } else { t - times_ms[i - 1] })
            .collect();

        // width for the elapsed time column (include total elapsed)
        let max_time = times_ms
            .iter()
            .copied()
            .chain(std::iter::once(elapsed.as_millis()))
            .max()
            .unwrap_or(0);
        let time_width = max_time.to_string().len();

        // width for the delta column
        let max_delta = deltas_ms.iter().copied().max().unwrap_or(0);
        let delta_width = max_delta.to_string().len();

        // width for the name column (ensure "Total elapsed" fits)
        let total_label = "Total elapsed";
        let name_width = self
            .events
            .iter()
            .map(|(_, e)| e.len())
            .max()
            .unwrap_or(0)
            .max(total_label.len());

        for ((_, event), (elapsed_ms, delta_ms)) in self
            .events
            .iter()
            .zip(times_ms.iter().zip(deltas_ms.iter()))
        {
            println!(
                "    {:name_width$}: {:>time_width$} ms  (+{:>delta_width$} ms)",
                event,
                *elapsed_ms,
                *delta_ms,
                name_width = name_width,
                time_width = time_width,
                delta_width = delta_width
            );
        }

        println!(
            "    {:name_width$}: {:>time_width$} ms",
            total_label,
            elapsed.as_millis(),
            name_width = name_width,
            time_width = time_width
        );
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        self.print();
    }
}
