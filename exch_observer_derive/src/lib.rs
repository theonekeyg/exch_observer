extern crate proc_macro;
use proc_macro::{
    TokenStream
};
use quote::quote;
use syn;

/// Macro for deriving ExchangeObserver trait.
/// Most WebSocket observers have few differences in their implementation,
/// there is no need to have separate implementations for each of them.
/// This derive macro is used to solve this problem of having multiple
/// exact implementations of the same trait.
#[proc_macro_derive(ExchangeObserver)]
pub fn impl_observer_derive(input: TokenStream) -> TokenStream {
    let ast = syn::parse(input).unwrap();

    impl_observer_macro(&ast)
}

/// Implementation of mostly optimzied observer macro.
/// It has some requirements for the struct that implements it,
/// all of them can be found in BinanceObserver implementation,
/// use it as a reference for new observers.
fn impl_observer_macro(ast: &syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl<Symbol> ExchangeObserver<Symbol> for #name<Symbol>
        where
            Symbol: Eq
                + Hash
                + Clone
                + Display
                + Debug
                + Into<String>
                + PairedExchangeSymbol
                + Send
                + Sync
                + 'static,
        {
            type Values = AskBidValues;

            fn get_interchanged_symbols(&self, symbol: &String) -> &'_ Vec<OrderedExchangeSymbol<Symbol>> {
                &self.connected_symbols.get::<String>(symbol).unwrap()
            }

            fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Self::Values>>) {
                let _symbol = <Symbol as Into<String>>::into(symbol.clone());
                if !self.price_table.contains_key(&_symbol) {
                    // Since with this design it's impossible to modify external ExchangeValues from
                    // thread, we're not required to lock the whole HashMap, since each thread modifies
                    // it's own *ExchangeValue.
                    //
                    // Unfortunately unsafe is required to achieve that in modern Rust, as there are no
                    // other options to expose that logic to the compiler.
                    unsafe {
                        let ptable_ptr = Arc::get_mut_unchecked(&mut self.price_table);
                        ptable_ptr.insert(_symbol.clone(), price);
                    }

                    if !self.connected_symbols.contains_key(symbol.base()) {
                        self.connected_symbols
                            .insert(symbol.base().to_string().clone(), Vec::new());
                    }

                    if !self.connected_symbols.contains_key(symbol.quote()) {
                        self.connected_symbols
                            .insert(symbol.quote().to_string().clone(), Vec::new());
                    }

                    self.connected_symbols
                        .get_mut(symbol.base())
                        .unwrap()
                        .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Sell));
                    self.connected_symbols
                        .get_mut(symbol.quote())
                        .unwrap()
                        .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Buy));

                    self.symbols_in_queue.push(symbol.clone());

                    if self.symbols_in_queue.len() >= self.symbols_queue_limit {
                        let thread_data = Arc::new(ObserverWorkerThreadData::from(
                            &self.symbols_in_queue,
                        ));

                        for sym in &self.symbols_in_queue {
                            self.threads_data_mapping
                                .insert(sym.clone(), thread_data.clone());
                        }

                        Self::launch_worker_multiple(
                            self.async_runner.deref(),
                            &self.symbols_in_queue,
                            self.price_table.clone(),
                            thread_data,
                        )
                        .unwrap();

                        self.symbols_in_queue.clear();
                    }

                    info!("Added {} to the watching symbols", &symbol);
                    self.watching_symbols.push(symbol.clone())
                }
            }

            fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<Self::Values>>> {
                let symbol = <Symbol as Into<String>>::into(symbol.clone());
                self.price_table.get(&symbol)
            }

            fn get_usd_value(&self, sym: &String) -> Option<f64> {
                // TODO: This lock USD wrapped tokens to 1 seems to be unnecessary,
                // considering to remove this later
                if let Some(_) = USD_STABLES.into_iter().find(|v| v == sym) {
                    return Some(1.0);
                };

                let connected = self.get_interchanged_symbols(sym);

                for ordered_sym in connected.iter() {
                    for stable in &USD_STABLES {
                        if <&str as Into<String>>::into(stable) == ordered_sym.symbol.base()
                            || <&str as Into<String>>::into(stable) == ordered_sym.symbol.quote()
                        {
                            return self.get_price_from_table(&ordered_sym.symbol).map(|v| {
                                let unlocked = v.lock().unwrap();
                                (unlocked.get_ask_price() + unlocked.get_bid_price()) / 2.0
                            });
                        }
                    }
                }

                None
            }

            fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                info!("Starting Binance Observer");

                if self.symbols_in_queue.len() > 0 {
                    let thread_data = Arc::new(ObserverWorkerThreadData::from(
                        &self.symbols_in_queue,
                    ));

                    for sym in &self.symbols_in_queue {
                        self.threads_data_mapping
                            .insert(sym.clone(), thread_data.clone());
                    }

                    Self::launch_worker_multiple(
                        self.async_runner.deref(),
                        &self.symbols_in_queue,
                        self.price_table.clone(),
                        thread_data,
                    )?;

                    self.symbols_in_queue.clear();
                }

                Ok(())
            }

            fn remove_symbol(&mut self, symbol: Symbol) {
                // Remove symbol from `watching_symbols`, `connected_symbols` from both base and quote
                // symbols and `price_table`, also vote for this symbol's thread to stop (won't stop
                // untill all symbols related to this thread vote to stop it).

                // Remove from the `watching_symbols`
                for (i, sym) in self.watching_symbols.iter().enumerate() {
                    if sym == &symbol {
                        self.watching_symbols.remove(i);
                        break;
                    }
                }

                // Remove from maptrees.
                let base_connected = self.connected_symbols.get_mut(symbol.base()).unwrap();
                for (i, sym) in base_connected.iter().enumerate() {
                    if sym.symbol == symbol {
                        base_connected.remove(i);
                        break;
                    }
                }

                let quote_connected = self.connected_symbols.get_mut(symbol.quote()).unwrap();
                for (i, sym) in quote_connected.iter().enumerate() {
                    if sym.symbol == symbol {
                        quote_connected.remove(i);
                        break;
                    }
                }

                // Mark the symbol to be removed from the worker thread, so it's price won't be updated
                // and it shows the thread that one symbol is removed.
                {
                    // This unsafe statement is safe because the only fields from the
                    // ObserverWorkerThreadData struct other thread uses is AtomicBool. However this way we
                    // won't have to deal with locks in this communication between threads. As long as
                    // only AtomicBool in this structure is used in subthreads, this is thread-safe.
                    unsafe {
                        let mut data = self
                            .threads_data_mapping
                            .get_mut(&symbol)
                            .unwrap();

                        let mut data = Arc::get_mut_unchecked(&mut data);

                        // Check that the symbol is not already marked to be removed.
                        if !data.requests_to_stop_map.get(&symbol).unwrap() {
                            data.requests_to_stop += 1;
                            data.requests_to_stop_map
                                .insert(symbol.clone(), true)
                                .unwrap();
                        }

                        // If all symbols are removed from the thread, stop it.
                        if data.requests_to_stop >= data.length {
                            // Stop the running thread.
                            data.stop_thread();

                            // Only remove prices when all symbols are removed from the thread.
                            for symbol in data.requests_to_stop_map.keys() {
                                // Remove symbol from `threads_data_mapping`

                                // Remove symbol from `price_table` This code is unsafe, since it gets
                                // mutable data directly from Arc. Although, this unsafe block should be
                                // safe, since we stopped the thread before.
                                let ptable_ptr = Arc::get_mut_unchecked(&mut self.price_table);
                                ptable_ptr.remove(&symbol.clone().into());
                            }
                        }
                    }
                }

                self.threads_data_mapping.remove(&symbol);
                debug!("Removed symbol {} from Binace observer", symbol);
            }

            fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
                return &self.watching_symbols;
            }
        }
    };
    gen.into()
}
