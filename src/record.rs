use std::collections::vec_deque::VecDeque;
use std::fmt::{self, Debug, Display, Formatter};
use std::marker::PhantomData;
use {Command, Error};

/// The signals sent when the record or the receiver changes.
///
/// When one of these states changes in the record or the receiver, they will send a corresponding
/// signal to the user. For example, if the record can no longer redo any commands, it sends a
/// `Signal::Redo(false)` signal to tell the user. The signals can be handled in the [`signals`]
/// method.
///
/// [`signals`]: struct.RecordBuilder.html#method.signals
#[derive(Copy, Clone, Debug, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum Signal {
    /// Says if the record can undo.
    ///
    /// This signal will be emitted when the records ability to undo changes.
    Undo(bool),
    /// Says if the record can redo.
    ///
    /// This signal will be emitted when the records ability to redo changes.
    Redo(bool),
    /// Says if the receiver is in a saved state.
    ///
    /// This signal will be emitted when the record enters or leaves its receivers saved state.
    Saved(bool),
    /// Says if the active command has changed.
    ///
    /// This signal will be emitted when the records active command has changed. This includes
    /// when two commands have been merged, in which case `old == new`.
    /// The `old` and `new` fields are `index of command + 1`.
    Active {
        /// The old command.
        old: usize,
        /// The new command.
        new: usize
    },
}

/// A record of commands.
///
/// The record works mostly like a stack, but it stores the commands
/// instead of returning them when undoing. This means it can roll the
/// receivers state backwards and forwards by using the undo and redo methods.
/// In addition, the record can notify the user about changes to the stack or
/// the receiver through [signals]. The user can give the record a function
/// that is called each time the state changes by using the [`builder`].
///
/// # Examples
/// ```
/// # use std::error::Error;
/// # use std::fmt::{self, Display, Formatter};
/// # use redo::*;
/// #[derive(Debug)]
/// struct MyError(&'static str);
///
/// // impl Display + Error for MyError..
/// # impl Display for MyError {
/// #    fn fmt(&self, f: &mut Formatter) -> fmt::Result { f.write_str(self.0) }
/// # }
/// # impl Error for MyError {
/// #    fn description(&self) -> &str { self.0 }
/// # }
///
/// #[derive(Debug)]
/// struct Add(char);
///
/// impl Command<String> for Add {
///     type Error = MyError;
///
///     fn apply(&mut self, s: &mut String) -> Result<(), Self::Error> {
///         s.push(self.0);
///         Ok(())
///     }
///
///     fn undo(&mut self, s: &mut String) -> Result<(), Self::Error> {
///         self.0 = s.pop().ok_or(MyError("`String` is unexpectedly empty"))?;
///         Ok(())
///     }
/// }
///
/// fn foo() -> Result<(), Box<Error>> {
///     let mut record = Record::default();
///
///     record.apply(Add('a'))?;
///     record.apply(Add('b'))?;
///     record.apply(Add('c'))?;
///
///     assert_eq!(record.as_receiver(), "abc");
///
///     record.undo().unwrap()?;
///     record.undo().unwrap()?;
///     record.undo().unwrap()?;
///
///     assert_eq!(record.as_receiver(), "");
///
///     record.redo().unwrap()?;
///     record.redo().unwrap()?;
///     record.redo().unwrap()?;
///
///     assert_eq!(record.into_receiver(), "abc");
///
///     Ok(())
/// }
/// # foo().unwrap();
/// ```
///
/// [`builder`]: struct.RecordBuilder.html
/// [signals]: enum.Signal.html
pub struct Record<'a, R, C: Command<R>> {
    commands: VecDeque<C>,
    receiver: R,
    cursor: usize,
    limit: usize,
    saved: Option<usize>,
    signals: Option<Box<FnMut(Signal) + Send + Sync + 'a>>,
}

impl<'a, R, C: Command<R>> Record<'a, R, C> {
    /// Returns a new record.
    #[inline]
    pub fn new<T: Into<R>>(receiver: T) -> Record<'a, R, C> {
        Record {
            commands: VecDeque::new(),
            receiver: receiver.into(),
            cursor: 0,
            limit: 0,
            saved: Some(0),
            signals: None,
        }
    }

    /// Returns a builder for a record.
    #[inline]
    pub fn builder() -> RecordBuilder<'a, R, C> {
        RecordBuilder {
            commands: PhantomData,
            receiver: PhantomData,
            capacity: 0,
            limit: 0,
            signals: None,
        }
    }

    /// Returns the capacity of the record.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.commands.capacity()
    }

    /// Returns the number of commands in the record.
    #[inline]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns `true` if the record is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Returns the limit of the record, or `None` if it has no limit.
    #[inline]
    pub fn limit(&self) -> Option<usize> {
        match self.limit {
            0 => None,
            v => Some(v)
        }
    }

    /// Returns `true` if the record can undo.
    #[inline]
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    /// Returns `true` if the record can redo.
    #[inline]
    pub fn can_redo(&self) -> bool {
        self.cursor < self.len()
    }

    /// Marks the receiver as currently being in a saved state.
    #[inline]
    pub fn set_saved(&mut self) {
        let was_saved = self.is_saved();
        self.saved = Some(self.cursor);
        if let Some(ref mut f) = self.signals {
            // Check if the receiver went from unsaved to saved.
            if !was_saved {
                f(Signal::Saved(true));
            }
        }
    }

    /// Marks the receiver as no longer being in a saved state.
    #[inline]
    pub fn set_unsaved(&mut self) {
        let was_saved = self.is_saved();
        self.saved = None;
        if let Some(ref mut f) = self.signals {
            // Check if the receiver went from saved to unsaved.
            if was_saved {
                f(Signal::Saved(false));
            }
        }
    }

    /// Returns `true` if the receiver is in a saved state, `false` otherwise.
    #[inline]
    pub fn is_saved(&self) -> bool {
        self.saved.map_or(false, |saved| saved == self.cursor)
    }

    /// Removes all commands from the record without undoing them.
    ///
    /// This resets the record back to its initial state and emits the appropriate signals,
    /// while leaving the receiver unmodified.
    #[inline]
    pub fn clear(&mut self) {
        let could_undo = self.can_undo();
        let could_redo = self.can_redo();
        let was_saved = self.is_saved();

        let old = self.cursor;
        self.commands.clear();
        self.cursor = 0;
        self.saved = Some(0);

        if let Some(ref mut f) = self.signals {
            // Emit signal if the cursor has changed.
            if old != 0 {
                f(Signal::Active { old, new: 0 });
            }
            // Record can never undo after being cleared, check if you could undo before.
            if could_undo {
                f(Signal::Undo(false));
            }
            // Record can never redo after being cleared, check if you could redo before.
            if could_redo {
                f(Signal::Redo(false));
            }
            // Check if the receiver went from unsaved to saved.
            if !was_saved {
                f(Signal::Saved(true));
            }
        }
    }

    /// Pushes the command on top of the record and executes its [`apply`] method.
    /// The command is merged with the previous top command if [`merge`] does not return `None`.
    ///
    /// All commands above the active one are removed from the stack and returned as an iterator.
    ///
    /// # Errors
    /// If an error occur when calling [`apply`] or when [merging commands][`merge`],
    /// the error is returned together with the command.
    ///
    /// [`apply`]: trait.Command.html#tymethod.apply
    /// [`merge`]: trait.Command.html#method.merge
    #[inline]
    pub fn apply(&mut self, mut cmd: C) -> Result<impl Iterator<Item=C>, Error<R, C>> {
        match cmd.apply(&mut self.receiver) {
            Ok(_) => {
                let old = self.cursor;
                let could_undo = self.can_undo();
                let could_redo = self.can_redo();
                let was_saved = self.is_saved();

                // Pop off all elements after len from record.
                let iter = self.commands.split_off(self.cursor).into_iter();
                debug_assert_eq!(self.cursor, self.len());

                // Check if the saved state was popped off.
                if self.saved.map_or(false, |saved| saved > self.cursor) {
                    self.saved = None;
                }

                let cmd = match self.commands.back_mut() {
                    Some(ref mut last) if !was_saved => last.merge(cmd).err(),
                    _ => Some(cmd),
                };

                if let Some(cmd) = cmd {
                    if self.limit != 0 && self.limit == self.cursor {
                        self.commands.pop_front();
                        self.saved = self.saved.and_then(|saved| saved.checked_sub(1));
                    } else {
                        self.cursor += 1;
                    }
                    self.commands.push_back(cmd);
                }

                debug_assert_eq!(self.cursor, self.len());
                if let Some(ref mut f) = self.signals {
                    // We emit this signal even if the commands might have been merged.
                    f(Signal::Active { old, new: self.cursor });
                    // Record can never redo after executing a command, check if you could redo before.
                    if could_redo {
                        f(Signal::Redo(false));
                    }
                    // Record can always undo after executing a command, check if you could not undo before.
                    if !could_undo {
                        f(Signal::Undo(true));
                    }
                    // Check if the receiver went from saved to unsaved.
                    if was_saved {
                        f(Signal::Saved(false));
                    }
                }
                Ok(iter)
            }
            Err(e) => Err(Error(cmd, e)),
        }
    }

    /// Calls the [`undo`] method for the active command and sets the previous one as the new active one.
    ///
    /// # Errors
    /// If an error occur when executing [`undo`] the error is returned and the state is left unchanged.
    ///
    /// [`undo`]: ../trait.Command.html#tymethod.undo
    #[inline]
    pub fn undo(&mut self) -> Option<Result<(), C::Error>> {
        if !self.can_undo() {
            return None;
        }

        let result = self.commands[self.cursor - 1].undo(&mut self.receiver).map(|_| {
            let was_saved = self.is_saved();
            let old = self.cursor;
            self.cursor -= 1;
            let len = self.len();
            let is_saved = self.is_saved();
            if let Some(ref mut f) = self.signals {
                // Cursor has always changed at this point.
                f(Signal::Active { old, new: self.cursor });
                // Check if the records ability to redo changed.
                if old == len {
                    f(Signal::Redo(true));
                }
                // Check if the records ability to undo changed.
                if old == 1 {
                    f(Signal::Undo(false));
                }
                // Check if the receiver went from saved to unsaved, or unsaved to saved.
                if was_saved != is_saved {
                    f(Signal::Saved(is_saved));
                }
            }
        });
        Some(result)
    }

    /// Calls the [`apply`] method for the active command and sets the next one as the new
    /// active one.
    ///
    /// # Errors
    /// If an error occur when executing [`apply`] the
    /// error is returned and the state is left unchanged.
    ///
    /// [`apply`]: trait.Command.html#tymethod.apply
    #[inline]
    pub fn redo(&mut self) -> Option<Result<(), C::Error>> {
        if !self.can_redo() {
            return None;
        }

        let result = self.commands[self.cursor].apply(&mut self.receiver).map(|_| {
            let was_saved = self.is_saved();
            let old = self.cursor;
            self.cursor += 1;
            let len = self.len();
            let is_saved = self.is_saved();
            if let Some(ref mut f) = self.signals {
                // Cursor has always changed at this point.
                f(Signal::Active { old, new: self.cursor });
                // Check if the records ability to redo changed.
                if old == len - 1 {
                    f(Signal::Redo(false));
                }
                // Check if the records ability to undo changed.
                if old == 0 {
                    f(Signal::Undo(true));
                }
                // Check if the receiver went from saved to unsaved, or unsaved to saved.
                if was_saved != is_saved {
                    f(Signal::Saved(is_saved));
                }
            }
        });
        Some(result)
    }

    /// Returns a reference to the `receiver`.
    #[inline]
    pub fn as_receiver(&self) -> &R {
        &self.receiver
    }

    /// Consumes the record, returning the `receiver`.
    #[inline]
    pub fn into_receiver(self) -> R {
        self.receiver
    }

    /// Returns an iterator over the commands.
    #[inline]
    pub fn commands(&self) -> impl Iterator<Item = &C> {
        self.commands.iter()
    }
}

impl<'a, R, C: Command<R> + ToString> Record<'a, R, C> {
    /// Returns the string of the command which will be undone in the next call to [`undo`].
    ///
    /// [`undo`]: struct.Record.html#method.undo
    #[inline]
    pub fn to_undo_string(&self) -> Option<String> {
        if self.can_undo() {
            Some(self.commands[self.cursor - 1].to_string())
        } else {
            None
        }
    }

    /// Returns the string of the command which will be redone in the next call to [`redo`].
    ///
    /// [`redo`]: struct.Record.html#method.redo
    #[inline]
    pub fn to_redo_string(&self) -> Option<String> {
        if self.can_redo() {
            Some(self.commands[self.cursor].to_string())
        } else {
            None
        }
    }
}

impl<'a, R: Default, C: Command<R>> Default for Record<'a, R, C> {
    #[inline]
    fn default() -> Record<'a, R, C> {
        Record::new(R::default())
    }
}

impl<'a, R, C: Command<R>> AsRef<R> for Record<'a, R, C> {
    #[inline]
    fn as_ref(&self) -> &R {
        self.as_receiver()
    }
}

impl<'a, R, C: Command<R>> From<R> for Record<'a, R, C> {
    #[inline]
    fn from(receiver: R) -> Self {
        Record::new(receiver)
    }
}

impl<'a, R: Debug, C: Command<R> + Debug> Debug for Record<'a, R, C> {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Record")
            .field("commands", &self.commands)
            .field("receiver", &self.receiver)
            .field("cursor", &self.cursor)
            .field("limit", &self.limit)
            .field("saved", &self.saved)
            .finish()
    }
}

impl<'a, R, C: Command<R> + Display> Display for Record<'a, R, C> {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for (i, cmd) in self.commands.iter().enumerate().rev() {
            if i + 1 == self.cursor {
                writeln!(f, "* {}", cmd)?;
            } else {
                writeln!(f, "  {}", cmd)?;
            }
        }
        Ok(())
    }
}

/// Builder for a record.
pub struct RecordBuilder<'a, R, C: Command<R>> {
    commands: PhantomData<C>,
    receiver: PhantomData<R>,
    capacity: usize,
    limit: usize,
    signals: Option<Box<FnMut(Signal) + Send + Sync + 'a>>,
}

impl<'a, R, C: Command<R>> RecordBuilder<'a, R, C> {
    /// Sets the [capacity] for the record.
    ///
    /// [capacity]: https://doc.rust-lang.org/std/vec/struct.Vec.html#capacity-and-reallocation
    #[inline]
    pub fn capacity(mut self, capacity: usize) -> RecordBuilder<'a, R, C> {
        self.capacity = capacity;
        self
    }

    /// Sets the `limit` for the record.
    ///
    /// If this limit is reached it will start popping of commands at the beginning
    /// of the record when pushing new commands on to the stack. No limit is set by
    /// default which means it may grow indefinitely.
    ///
    /// # Examples
    /// ```
    /// # use std::error::Error;
    /// # use std::fmt::{self, Display, Formatter};
    /// # use redo::*;
    /// #
    /// # #[derive(Debug)]
    /// # struct MyError(&'static str);
    /// #
    /// # impl Display for MyError {
    /// #     fn fmt(&self, f: &mut Formatter) -> fmt::Result { write!(f, "{}", self.0) }
    /// # }
    /// #
    /// # impl Error for MyError {
    /// #     fn description(&self) -> &str { self.0 }
    /// # }
    /// #
    /// # #[derive(Debug)]
    /// # struct Add(char);
    /// #
    /// # impl Command<String> for Add {
    /// #     type Error = MyError;
    /// #
    /// #     fn apply(&mut self, s: &mut String) -> Result<(), MyError> {
    /// #         s.push(self.0);
    /// #         Ok(())
    /// #     }
    /// #
    /// #     fn undo(&mut self, s: &mut String) -> Result<(), MyError> {
    /// #         self.0 = s.pop().ok_or(MyError("`String` is unexpectedly empty"))?;
    /// #         Ok(())
    /// #     }
    /// # }
    /// #
    /// # fn foo() -> Result<(), Box<Error>> {
    /// let mut record = Record::builder()
    ///     .capacity(2)
    ///     .limit(2)
    ///     .default();
    ///
    /// record.apply(Add('a'))?;
    /// record.apply(Add('b'))?;
    /// record.apply(Add('c'))?; // 'a' is removed from the record since limit is 2.
    ///
    /// assert_eq!(record.as_receiver(), "abc");
    ///
    /// record.undo().unwrap()?;
    /// record.undo().unwrap()?;
    /// assert!(record.undo().is_none());
    ///
    /// assert_eq!(record.into_receiver(), "a");
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    #[inline]
    pub fn limit(mut self, limit: usize) -> RecordBuilder<'a, R, C> {
        self.limit = limit;
        self
    }

    /// Decides how different signals should be handled when the state changes.
    /// By default the record does nothing.
    ///
    /// # Examples
    /// ```
    /// # use std::error::Error;
    /// # use std::fmt::{self, Display, Formatter};
    /// # use redo::*;
    /// #
    /// # #[derive(Debug)]
    /// # struct MyError(&'static str);
    /// #
    /// # impl Display for MyError {
    /// #     fn fmt(&self, f: &mut Formatter) -> fmt::Result { write!(f, "{}", self.0) }
    /// # }
    /// #
    /// # impl Error for MyError {
    /// #     fn description(&self) -> &str { self.0 }
    /// # }
    /// #
    /// # #[derive(Debug)]
    /// # struct Add(char);
    /// #
    /// # impl Command<String> for Add {
    /// #     type Error = MyError;
    /// #
    /// #     fn apply(&mut self, s: &mut String) -> Result<(), MyError> {
    /// #         s.push(self.0);
    /// #         Ok(())
    /// #     }
    /// #
    /// #     fn undo(&mut self, s: &mut String) -> Result<(), MyError> {
    /// #         self.0 = s.pop().ok_or(MyError("`String` is unexpectedly empty"))?;
    /// #         Ok(())
    /// #     }
    /// # }
    /// #
    /// # fn foo() -> Result<(), Box<Error>> {
    /// # let mut record =
    /// Record::builder()
    ///     .signals(|signal| {
    ///         match signal {
    ///             Signal::Undo(true) => println!("The record can undo."),
    ///             Signal::Undo(false) => println!("The record can not undo."),
    ///             Signal::Redo(true) => println!("The record can redo."),
    ///             Signal::Redo(false) => println!("The record can not redo."),
    ///             Signal::Saved(true) => println!("The receiver is in a saved state."),
    ///             Signal::Saved(false) => println!("The receiver is not in a saved state."),
    ///             Signal::Active { old, new } => {
    ///                 println!("The active command has changed from {} to {}.", old, new);
    ///             }
    ///         }
    ///     })
    ///     .default();
    /// # record.apply(Add('a'))?;
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    #[inline]
    pub fn signals<F>(mut self, f: F) -> RecordBuilder<'a, R, C>
        where
            F: FnMut(Signal) + Send + Sync + 'a,
    {
        self.signals = Some(Box::new(f));
        self
    }

    /// Creates the record.
    #[inline]
    pub fn build<T: Into<R>>(self, receiver: T) -> Record<'a, R, C> {
        Record {
            commands: VecDeque::with_capacity(self.capacity),
            receiver: receiver.into(),
            cursor: 0,
            limit: self.limit,
            saved: Some(0),
            signals: self.signals,
        }
    }
}

impl<'a, R: Default, C: Command<R>> RecordBuilder<'a, R, C> {
    /// Creates the record with a default `receiver`.
    #[inline]
    pub fn default(self) -> Record<'a, R, C> {
        self.build(R::default())
    }
}

impl<'a, R: Debug, C: Command<R> + Debug> Debug for RecordBuilder<'a, R, C> {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("RecordBuilder")
            .field("receiver", &self.receiver)
            .field("capacity", &self.capacity)
            .field("limit", &self.limit)
            .finish()
    }
}
