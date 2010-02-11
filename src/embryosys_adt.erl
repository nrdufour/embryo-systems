%% Copyright 2009-2010 Nicolas R Dufour.
%%
%% Licensed under the Apache License, Version 2.0 (the "License");
%% you may not use this file except in compliance with the License.
%% You may obtain a copy of the License at
%%
%%     http://www.apache.org/licenses/LICENSE-2.0
%%
%% Unless required by applicable law or agreed to in writing, software
%% distributed under the License is distributed on an "AS IS" BASIS,
%% WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
%% See the License for the specific language governing permissions and
%% limitations under the License.
%%
%% @author Nicolas R Dufour <nrdufour@gmail.com>
%% @copyright 2009-2010 Nicolas R Dufour.

%% @type adtOperation() = create | hibern | awake | destroy | resur | purge
%% @type adtState() = alive | frozen | destroyed | none

-module(embryosys_adt).
-author('Nicolas R Dufour <nrdufour@gmail.com>').
-include("adt.hrl").

-export([new_adt/2, new_state_after/2, is_ready_for/2, get_adt_state/2, set_adt_state/3]).

new_adt(Type, Names) ->
    case Type of
        class     ->
            [ClassName] = Names,
            #class{ name = ClassName, state = alive };
        attribute ->
            [ClassName, AttributeName] = Names,
            #attribute{ name = AttributeName, state = alive, class = ClassName };
        link      ->
            [From,To,LinkName] = Names,
            #link{ name = LinkName, state = alive, from = From, to = To };
        object    ->
            [ClassName, ObjectName] = Names,
            #object{ name = ObjectName, state = alive, class = ClassName };
        _         -> throw(invalid_type)
    end.

%% @spec new_state_after(adtOperation(), adtState()) -> adtState()
%% @doc returns the ADT state following a given operation.
new_state_after(Operation, State) ->
    case {Operation, State} of
        {create, none}   -> alive;
        {hibern, alive}    -> frozen;
        {awake, frozen}    -> alive;
        {destroy, alive}   -> destroyed;
        {resur, destroyed} -> alive;
        {purge, destroyed} -> none;
        {_, _}             -> wrong_state
    end.

%% @spec is_ready_for(adtOperation(), adtState()) -> boolean()
%% @doc returns true if you can perform a given operation on an Adt
%% with the given state.
is_ready_for(Operation, State) ->
    new_state_after(Operation, State) /= wrong_state.

%% @spec get_adt_state(adtType(), adt()) -> adt()
%% @doc returns the current state of a given Adt.
get_adt_state(Type, Adt) ->
    case Type of
        class     -> Adt#class.state;
        attribute -> Adt#attribute.state;
        link      -> Adt#link.state;
        object    -> Adt#object.state;
        _         ->throw(invalid_type)
    end.

%% @spec set_adt_state(adtType(), adt(), adtState()) -> adt()
%% @doc set the Adt state.
set_adt_state(Type, Adt, NewState) ->
    case Type of
        class     -> Adt#class{state = NewState};
        attribute -> Adt#attribute{state = NewState};
        link      -> Adt#link{state = NewState};
        object    -> Adt#object{state = NewState};
        _         -> throw(invalid_type)
    end.

%%
