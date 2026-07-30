use opentelemetry::propagation::Injector;

pub struct MessageHeaderInjector<'a>(pub &'a mut async_nats::HeaderMap);

impl<'a> Injector for MessageHeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key, value);
    }
}
