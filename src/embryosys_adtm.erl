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

-module(embryosys_adtm).
-author('Nicolas R Dufour <nrdufour@gmail.com>').
-include("adt.hrl").

%% API exports
-export([do_create/2, do_hibern/2, do_awake/2, do_destroy/2, do_resur/2, do_purge/2]).

do_create(Type, Names) ->
    do_it(create, Type, Names).

do_hibern(Type, Names) ->
    do_it(hibern, Type, Names).

do_awake(Type, Names) ->
    do_it(awake, Type, Names).

do_destroy(Type, Names) ->
    do_it(destroy, Type, Names).

do_resur(Type, Names) ->
    do_it(resur, Type, Names).

do_purge(Type, Names) ->
    do_it(purge, Type, Names).

%%% --------------------------------------------------------------------------

is_class_alive(ClassName) ->
    ClassAdt = embryosys_storage_server:load(class, [ClassName]),
    case ClassAdt of
        not_found -> false;
        _         -> Meta = ClassAdt#adt.meta,
		     (Meta#meta.type =:= class) and (Meta#meta.state =:= alive)
    end.

are_adt_parents_valid(class, _Names) ->
    true;
are_adt_parents_valid(Type, Names) when Type == attribute; Type == object ->
    [ClassName, _Name] = Names,
    is_class_alive(ClassName);
are_adt_parents_valid(link, Names) ->
    [FromClassName, ToClassName, _LinkName] = Names,
    is_class_alive(FromClassName) and is_class_alive(ToClassName);
are_adt_parents_valid(_Type, _Names) ->
    throw(invalid_type).

check_adt_state(Type, Names, Operation) ->
    AreParentsValid = are_adt_parents_valid(Type, Names),
    case AreParentsValid of
        false ->
            {error, invalid_parents};
	_     ->
            Adt = embryosys_storage_server:load(Type, Names),
            case Operation of
                create ->
                    case Adt of
                        not_found -> {adt, not_found, alive};
                        _         -> {error, already_exists}
                    end;
                _      ->
                    case Adt of
                        not_found -> {error, not_found};
                        _         -> 
                            Meta = Adt#adt.meta,
                            NextState = embryosys_adt:new_state_after(Operation, Meta#meta.state),
                            case NextState of
                                wrong_state -> {error, wrong_state};
                                _           -> {adt, Adt, NextState}
                            end
                    end
            end
    end.

do_it(Operation, Type, Names) ->
    Check = check_adt_state(Type, Names, Operation),
    case Check of
        {error, _Reason} -> Check;
	{adt, Adt, NextState} ->
            case Operation of
                create  ->
                    NewAdt = embryosys_adt:new_adt(Type, Names),
                    embryosys_storage_server:store(Type, Names, NewAdt),
		    {ok, alive};
                hibern ->
                    Meta = Adt#adt.meta,
		    UpdatedMeta = Meta#meta{state = NextState},
		    UpdatedAdt = Adt#adt{meta = UpdatedMeta},
		    embryosys_storage_server:store(Type, Names, UpdatedAdt),
		    {ok, frozen};
                awake   ->
                    Meta = Adt#adt.meta,
		    UpdatedMeta = Meta#meta{state = NextState},
		    UpdatedAdt = Adt#adt{meta = UpdatedMeta},
		    embryosys_storage_server:store(Type, Names, UpdatedAdt),
		    {ok, alive};
                destroy ->
                    Meta = Adt#adt.meta,
		    UpdatedMeta = Meta#meta{state = NextState},
		    UpdatedAdt = Adt#adt{meta = UpdatedMeta},
		    embryosys_storage_server:store(Type, Names, UpdatedAdt),
		    {ok, destroyed};
                resur   ->
                    Meta = Adt#adt.meta,
		    UpdatedMeta = Meta#meta{state = NextState},
		    UpdatedAdt = Adt#adt{meta = UpdatedMeta},
		    embryosys_storage_server:store(Type, Names, UpdatedAdt),
		    {ok, alive};
                purge   ->
		    {error, not_yet_implemented};
                _       ->
		    {error, wrong_operation}
            end
    end.

%%

