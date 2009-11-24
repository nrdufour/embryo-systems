%% @author Nicolas R Dufour <nrdufour@gmail.com>
%% @copyright 2009 Nicolas R Dufour.
%%
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

-module(embryosys_util).
-author('Nicolas R Dufour <nrdufour@gmail.com>').

-export([new_state_after/2, is_ready_for/2, new_uuid/0]).

-include("adt.hrl").

%% State transition grid (strict)
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

is_ready_for(Operation, State) ->
	new_state_after(Operation, State) /= wrong_state.


%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%% Piece of code from couch_util.erl in Apache CouchDB project.
new_uuid() ->
    list_to_binary(to_hex(crypto:rand_bytes(16))).

to_hex([]) ->
    [];
to_hex(Bin) when is_binary(Bin) ->
    to_hex(binary_to_list(Bin));
to_hex([H|T]) ->
    [to_digit(H div 16), to_digit(H rem 16) | to_hex(T)].

to_digit(N) when N < 10 -> $0 + N;
to_digit(N)             -> $a + N-10.
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%

%%
