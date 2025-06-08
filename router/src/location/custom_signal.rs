use std::marker::PhantomData;

pub struct CustomSignal<T> {
    test: PhantomData<T>,
}
