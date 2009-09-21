/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.manager.impl;

import info.embryosys.adt.core.Adt;
import info.embryosys.adt.core.AdtFactory;
import info.embryosys.adt.core.AdtId;
import info.embryosys.adt.core.AdtState;
import info.embryosys.adt.core.AdtType;
import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.Workspace;
import info.embryosys.adt.request.AdtRequest;
import info.embryosys.adt.storage.Storage;
import info.embryosys.adt.storage.impl.DummyStorage;

/**
 * @author nrdufour
 * 
 */
public class SimpleAdtManager implements AdtManager {

	private Workspace workspace;

	/**
	 * @return the workspace
	 */
	public Workspace getWorkspace() {
		return workspace;
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.AdtManager#init()
	 */
	public void init() {
		// FIXME for now it's hardcoded .. I will find a way to configure that
		// through a conf file

		this.workspace = new Workspace("default", new DummyStorage());
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see
	 * info.embryosys.adt.manager.AdtManager#processRequest(info.embryosys.adt
	 * .manager.AdtRequest)
	 */
	public void processRequest(final AdtRequest request) {
		Storage storage = workspace.getStorage();

		System.out.println("Processing request: " + request);

		// FIXME must check the existence of the parents
		if (!checkIfParentsExist(storage, request)) {
			throw new IllegalArgumentException("Parent classes missing !");
		}

		AdtId possibleId = storage.find(request.getType(), request.getName());

		switch (request.getOperation()) {
		case CREATE:
			create(storage, request, possibleId);
			break;
		case HIBERN:
		case AWAKE:
		case DESTROY:
		case RESUR:
			hadr(storage, request, possibleId);
			break;
		case PURGE:
			purge(storage, request, possibleId);
			break;
		case RENAME:
			rename(storage, request, possibleId);
			break;
		default:
		}
	}

	/**
	 * @param request
	 */
	private boolean checkIfParentsExist(Storage storage, AdtRequest request) {
		// FIXME for now, the parents are always of type class because we have
		// only CALO.
		for (String parent : request.getParents()) {
			AdtId parentId = storage.find(AdtType.CLASS, parent);
			if (parentId == null) {
				return false;
			}
		}

		return true;
	}

	/**
	 * @param storage
	 * @param request
	 */
	private void purge(Storage storage, AdtRequest request, AdtId possibleId) {
		// TODO Auto-generated method stub

	}

	/**
	 * @param storage
	 * @param request
	 */
	private void rename(Storage storage, AdtRequest request, AdtId possibleId) {
		// TODO Auto-generated method stub

	}

	/**
	 * @param storage
	 * @param request
	 */
	private void hadr(Storage storage, AdtRequest request, AdtId possibleId) {
		if (possibleId == null) {
			throw new RuntimeException("The ADT [" + request.getName()
					+ "] doesn't exist !");
		}

		Adt adt = storage.load(possibleId);

		// FIXME need to check that the CURRENT state is compatible here

		AdtState state = null;
		switch (request.getOperation()) {
		case HIBERN:
			state = AdtState.FROZEN;
			break;
		case AWAKE:
			state = AdtState.ALIVE;
			break;
		case DESTROY:
			state = AdtState.DESTROYED;
			break;
		case RESUR:
			state = AdtState.ALIVE;
			break;
		default:
		}

		if (state == null) {
			throw new RuntimeException(
					"Wrong state/Wrong Operation during HADR operation !");
		}

		adt.setState(state);

		storage.store(adt);
	}

	public void create(final Storage storage, final AdtRequest request,
			AdtId possibleId) {
		if (possibleId != null) {
			throw new RuntimeException("The ADT [" + request.getName()
					+ "] already exists as " + possibleId);
		}

		Adt adt = AdtFactory.createNewAdt(request.getType(), request.getName());

		adt.setState(AdtState.ALIVE);

		storage.store(adt);
	}
}
